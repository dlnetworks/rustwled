// Certificate Management Module - Auto-generate and manage self-signed SSL certificates
use anyhow::{Context, Result};
use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType, SanType};
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

/// Get the path to the config directory
fn config_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    let config_dir = PathBuf::from(home).join(".config").join("rustwled");
    fs::create_dir_all(&config_dir)?;
    Ok(config_dir)
}

/// Get paths to certificate files
pub fn cert_paths() -> Result<(PathBuf, PathBuf)> {
    let dir = config_dir()?;
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    Ok((cert_path, key_path))
}

/// Check if certificates exist and are valid
pub fn certs_exist() -> bool {
    if let Ok((cert_path, key_path)) = cert_paths() {
        cert_path.exists() && key_path.exists()
    } else {
        false
    }
}

/// Check if certificate is expired or close to expiring (within 30 days)
pub fn cert_needs_renewal() -> Result<bool> {
    let (cert_path, _) = cert_paths()?;

    if !cert_path.exists() {
        return Ok(true);
    }

    // For simplicity, we'll just regenerate certs older than 335 days (30 day buffer before 365 day expiry)
    let metadata = fs::metadata(&cert_path)?;
    if let Ok(modified) = metadata.modified() {
        if let Ok(duration) = SystemTime::now().duration_since(modified) {
            // Renew if cert is older than 335 days (30 days before 365 day expiry)
            return Ok(duration.as_secs() > 335 * 24 * 60 * 60);
        }
    }

    Ok(false)
}

/// Generate a new self-signed certificate
pub fn generate_certificate(hostname: &str) -> Result<()> {
    println!("\n🔐 Generating self-signed SSL certificate for: {}", hostname);

    let (cert_path, key_path) = cert_paths()?;

    // Create certificate parameters
    let mut params = CertificateParams::default();

    // Set subject alternative names (SANs) - include both the provided hostname and common variations
    params.subject_alt_names = vec![
        SanType::DnsName(hostname.to_string()),
    ];

    // If hostname is an IP address, add it as an IP SAN
    if let Ok(ip) = hostname.parse::<std::net::IpAddr>() {
        params.subject_alt_names.push(SanType::IpAddress(ip));
    }

    // Add common localhost variations if not already the hostname
    if hostname != "localhost" && hostname != "127.0.0.1" {
        params.subject_alt_names.push(SanType::DnsName("localhost".to_string()));
        params.subject_alt_names.push(SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))));
    }

    // Set distinguished name
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, hostname);
    dn.push(DnType::OrganizationName, "RustWLED Self-Signed");
    params.distinguished_name = dn;

    // Set validity period (365 days)
    params.not_before = time::OffsetDateTime::now_utc();
    params.not_after = time::OffsetDateTime::now_utc() + time::Duration::days(365);

    // Generate the certificate
    let cert = Certificate::from_params(params)?;

    // Get PEM-encoded certificate and private key
    let cert_pem = cert.serialize_pem()?;
    let key_pem = cert.serialize_private_key_pem();

    // Write to files
    fs::write(&cert_path, cert_pem)?;
    fs::write(&key_path, key_pem)?;

    println!("✅ Certificate generated successfully!");
    println!("   Cert: {:?}", cert_path);
    println!("   Key:  {:?}", key_path);
    println!("   Valid for: 365 days");
    println!("\n⚠️  Your browser will show a security warning because this is a self-signed certificate.");
    println!("   Click \"Advanced\" then \"Proceed to {} (unsafe)\" to continue.\n", hostname);

    Ok(())
}

/// Ensure certificates exist, generate if needed, prompt user for hostname if not configured
pub fn ensure_certificates(hostname: &str) -> Result<()> {
    // Check if hostname is configured
    if hostname.is_empty() {
        anyhow::bail!(
            "HTTPS enabled but no IP address configured.\n\
             Please set 'httpd_ip' in your config file to your server's IP address or hostname.\n\
             Example: httpd_ip = \"192.168.1.100\"\n\
             Then restart the application."
        );
    }

    // Check if certificates exist and are valid
    if !certs_exist() {
        println!("\n📜 No SSL certificates found. Generating new certificates...");
        generate_certificate(hostname)?;
    } else if cert_needs_renewal()? {
        println!("\n📜 SSL certificate is expiring soon. Regenerating...");
        generate_certificate(hostname)?;
    }

    Ok(())
}

/// Load certificate and key from files
pub fn load_certificates() -> Result<(Vec<u8>, Vec<u8>)> {
    let (cert_path, key_path) = cert_paths()?;

    let cert = fs::read(&cert_path)
        .with_context(|| format!("Failed to read certificate from {:?}", cert_path))?;
    let key = fs::read(&key_path)
        .with_context(|| format!("Failed to read private key from {:?}", key_path))?;

    Ok((cert, key))
}

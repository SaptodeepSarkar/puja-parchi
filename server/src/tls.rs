//! HTTPS for the phone.
//!
//! Chrome refuses the microphone and service-worker registration on a plain
//! http:// LAN origin, and on a click-through self-signed cert. So we mint a
//! local CA once, sign a leaf for the current LAN IPs, and the phone installs
//! the CA one time. The CA key never leaves this machine.

use std::net::{IpAddr, UdpSocket};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
};

pub struct Tls {
    pub cert: String,
    pub key: String,
    pub ca_path: PathBuf,
}

/// Every address the phone might type. Loopback + whatever the kernel says.
pub fn local_ips() -> Vec<IpAddr> {
    let mut ips = vec![IpAddr::from([127, 0, 0, 1])];
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = sock.local_addr() {
                if !addr.ip().is_loopback() && !ips.contains(&addr.ip()) {
                    ips.push(addr.ip());
                }
            }
        }
    }
    ips
}

fn ca_params() -> CertificateParams {
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(DnType::CommonName, "Puja Parchi Local CA (delete after the puja)");
    params.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign, rcgen::KeyUsagePurpose::CrlSign];
    params
}

pub fn ensure(data_dir: &Path) -> Result<Tls> {
    let dir = data_dir.join("certs");
    std::fs::create_dir_all(&dir)?;
    let ca_path = dir.join("ca.crt");
    let ca_key_path = dir.join("ca.key");

    // Reuse the CA so the phone's one-time trust survives restarts.
    let issuer = if ca_path.exists() && ca_key_path.exists() {
        let pem = std::fs::read_to_string(&ca_path)?;
        let key = KeyPair::from_pem(&std::fs::read_to_string(&ca_key_path)?).context("ca.key")?;
        Issuer::from_ca_cert_pem(&pem, key).context("ca.crt")?
    } else {
        let key = KeyPair::generate()?;
        let params = ca_params();
        std::fs::write(&ca_path, params.self_signed(&key)?.pem())?;
        std::fs::write(&ca_key_path, key.serialize_pem())?;
        tracing::info!("created local CA at {}", ca_path.display());
        Issuer::new(params, key)
    };

    // Fresh leaf every boot, so a new LAN IP is always covered.
    let mut names: Vec<String> = vec!["localhost".into(), "puja-parchi.local".into()];
    names.extend(local_ips().iter().map(ToString::to_string));
    let mut params = CertificateParams::new(names)?;
    params
        .distinguished_name
        .push(DnType::CommonName, "puja-parchi.local");
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];

    let leaf_key = KeyPair::generate()?;
    let leaf = params.signed_by(&leaf_key, &issuer)?;
    Ok(Tls { cert: leaf.pem(), key: leaf_key.serialize_pem(), ca_path })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_a_reusable_ca_and_a_fresh_leaf() {
        let tmp = tempfile::tempdir().unwrap();
        let a = ensure(tmp.path()).unwrap();
        assert!(a.ca_path.exists());
        assert!(a.cert.contains("BEGIN CERTIFICATE"));
        let b = ensure(tmp.path()).unwrap();
        let ca_a = std::fs::read_to_string(&a.ca_path).unwrap();
        let ca_b = std::fs::read_to_string(&b.ca_path).unwrap();
        assert_eq!(ca_a, ca_b, "CA must not rotate, or the phone has to trust it again");
        assert_ne!(a.cert, b.cert, "leaf should be re-signed each boot for the current IPs");
    }

    #[test]
    fn loopback_is_always_a_name() {
        assert!(local_ips().contains(&IpAddr::from([127, 0, 0, 1])));
    }
}

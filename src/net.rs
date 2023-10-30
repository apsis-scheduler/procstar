use native_tls::TlsConnector;

use crate::err::Error;

//------------------------------------------------------------------------------

pub fn get_tls_connector() -> Result<TlsConnector, Error> {
    let mut builder = native_tls::TlsConnector::builder();

    if let Some((_, cert_path)) = std::env::vars()
        .filter(|(n, _)| n == "PROCSTAR_CERT")
        .next()
    {
        let cert = std::fs::read_to_string(cert_path)?;
        let cert = native_tls::Certificate::from_pem(&cert.as_bytes())?;
        builder.add_root_certificate(cert);
    }

    Ok(builder.build().unwrap())
}

pub fn get_access_token() -> Option<String> {
    if let Some((_, token)) = std::env::vars()
        .filter(|(n, _)| n == "PROCSTAR_TOKEN")
        .next()
    {
        Some(token)
    } else {
        None
    }
}

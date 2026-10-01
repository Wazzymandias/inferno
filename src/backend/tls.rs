//! Shared Rustls crypto provider initialization.

pub(crate) fn install_crypto_provider() {
    // Reqwest needs the selected Rustls provider before constructing any client.
    static CRYPTO: std::sync::Once = std::sync::Once::new();
    CRYPTO.call_once(|| {
        rustls_graviola::default_provider()
            .install_default()
            .expect("install Graviola as the Rustls crypto provider");
    });
}

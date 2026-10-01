TLS test fixtures only. The private key here is public test data, never a deployment credential.

ca.der: self-signed RSA test CA, valid for 100 years from generation.
localhost.der: RSA server certificate signed by that CA, with DNS SAN localhost.
localhost-key.der: unencrypted PKCS#8 private key for localhost.der.

The CA is trusted only by the test client. Production uses the bundled WebPKI roots.
The absent IP SAN deliberately lets tests check rejection of a hostname mismatch.

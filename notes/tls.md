Created `localhost` self-signed CERT with this command from [letsencrypt.org](https://letsencrypt.org/docs/certificates-for-localhost/)

```
openssl req -x509 -days 3650 -out localhost.crt -keyout localhost.key \
  -newkey rsa:2048 -nodes -sha256 \
  -subj '/CN=localhost' -extensions EXT -config <( \
   printf "[dn]\nCN=localhost\n[req]\ndistinguished_name = dn\n[EXT]\nsubjectAltName=DNS:localhost\nkeyUsage=digitalSignature\nextendedKeyUsage=serverAuth")
```

To use this (or another) cert with procstar:
```
PROCSTAR_AGENT_CERT=path/to/cert.crt procstar ...
```


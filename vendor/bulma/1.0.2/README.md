# Bulma 1.0.2

`bulma.min.css` is `css/bulma.min.css` from the npm package `bulma@1.0.2`,
unmodified (MIT, see `LICENSE`). `build.rs` embeds it and serves it at a
content-hashed URL.

- Tarball: https://registry.npmjs.org/bulma/-/bulma-1.0.2.tgz
  (`sha512-D7GnDuF6seb6HkcnRMM9E739QpEY9chDzzeFrHMyEns/EXyDJuQ0XA0KxbBl/B2NTsKSoDomW61jFGFaAxhK5A==`)
- `bulma.min.css`: `sha384-tl5h4XuWmVzPeVWU0x8bx0j/5iMwCBduLEgZ+2lH4Wjda+4+q3mpCww74dgAB3OX`

To check or replace it:

```sh
curl -sL "$(curl -s https://registry.npmjs.org/bulma/1.0.2 | jq -r .dist.tarball)" | tar xz
openssl dgst -sha384 -binary package/css/bulma.min.css | openssl base64 -A
```

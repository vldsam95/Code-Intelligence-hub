# Public source release

The project is distributed under the MIT License. Preserve the copyright and
permission notice when copying or redistributing substantial portions. Attribution
is to [vldsam95 / Code-Intelligence-hub](https://github.com/vldsam95/Code-Intelligence-hub).
Contributions are welcome under the same license.

## Deployment examples

`code-hub.example.com` is a reserved example hostname, not an operated public Hub.
Replace it with your own hostname in `.env`, the Nginx example and project smoke
policy. Configure TLS, an external master key and unique credentials before deployment.
The example Nginx configuration is `deploy/nginx/code-hub.example.com.conf`.
The reference checkout location is `/opt/code-intelligence`.

The committed project manifest uses an example identity. For an independent copy,
use `codeintel fork-project` rather than binding to an unrelated existing Hub project.
Historical verification metadata is marked STALE; rebuild the graph against the current
source revision before using it as evidence.

## Dependency notices

Run the offline check before building distributions:

```bash
python scripts/third-party-licenses.py --check
```

If dependencies change, intentionally refresh notices, review the diff and check again:

```bash
python scripts/third-party-licenses.py --refresh
python scripts/third-party-licenses.py --check
```

Refresh downloads checksum-verified public source archives and copies notices only.
It does not install or execute third-party packages. Ship `LICENSE`,
`THIRD_PARTY_NOTICES.md` and `licenses/` with every distribution. The release script
and Hub image retain these files. Source archives linked in the inventory provide
the corresponding MPL-covered source. See [third-party notices](../THIRD_PARTY_NOTICES.md).

## Validation of this source update

The English UI/documentation update passed all 21 Python tests, Python lint,
JavaScript syntax checks, release-shell syntax checking, local Markdown file-link
checking and the offline inventory check for 270 dependency versions.
The application wheel was also built with its license files included.
This is validation of the source update, not a new production deployment or a
signed binary release. Full Linux/Rust/container builds require the documented toolchain.

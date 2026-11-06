# Third-party notices

Code Intelligence Platform's original code is licensed under the [MIT License](LICENSE).
Third-party components retain their own licenses; the project license does not replace them.

## Dependencies included in distributions

The complete notice collection and corresponding source archive links are in
[licenses/README.md](licenses/README.md). The machine-readable inventory is
`licenses/third-party/manifest.json`. It records versions, archive checksums, notice-file
checksums and pinned upstream notice URLs where a package archive omits its license files.
The inventory conservatively includes optional, development and target-specific packages,
not all of which ship in every build.

Dependency inputs are `worker/Cargo.lock`, `uv.lock` and `hub/package-lock.json`.
Worker releases use Cargo's `--locked`; Hub images use Python constraints generated from
`uv.lock` and `npm ci`. License inventory validation is an offline release prerequisite.
Refresh intentionally with `python scripts/third-party-licenses.py --refresh`, review the
results, then run `python scripts/third-party-licenses.py --check`.

Worker, Hub, adapter, documentation and complete-kit archives include this file,
the project license and the `licenses/` directory. Hub images also retain them.

- MIT, Apache-2.0, BSD, ISC, Unicode and other notices are retained in full as supplied
  by dependency authors. Dual-license alternatives remain identified by their notices.
- `ring 0.17.9` has composite licensing, including ISC, OpenSSL/SSLeay and MIT material.
  Its full published license is included. This product includes software developed by
  the OpenSSL Project for use in the OpenSSL Toolkit (https://www.openssl.org/),
  cryptographic software written by Eric Young, and software written by Tim Hudson.
- `webpki-roots 0.25.4` and `certifi 2026.7.22` are MPL-2.0 components. Their unchanged
  corresponding source archives are linked in `licenses/README.md` and the manifest;
  recipients may obtain and use that covered source under the included MPL terms.
  The project's MIT license does not restrict those rights.
- Legacy `winapi-*-pc-windows-gnu 0.4.0` import-library packages omit notices from their
  crates; notices are retained from the pinned winapi-rs 0.3.9 repository.
  `system-configuration-sys 0.5.0` notices use the published upstream v0.5.0 tag because
  its embedded development revision is no longer reachable upstream.

Operating-system libraries and base-image packages in redistributed container images
retain their upstream licenses and notices. This inventory covers application
dependencies, not every package in a base image.

## Acknowledgements — design and capability references

These projects informed capability requirements. They are not runtime services or
vendored source dependencies. The inventory above covers actual packaged libraries.

| Project | Author/organization | License | Reference revision reviewed |
| --- | --- | --- | --- |
| [codegraph](https://github.com/colbymchenry/codegraph) | Colby Mchenry | MIT | `c6aaa20358cd6adcd04b87bdef8e5803ad146f3a` |
| [codebase-memory-mcp](https://github.com/DeusData/codebase-memory-mcp) | DeusData | MIT | `4d7d9f1117b8e0c9322ff8c73e5124b0b5f7a185` |
| [code-review-graph](https://github.com/tirth8205/code-review-graph) | Tirth Kanani | MIT | `0f68cb0823d5aace65262d1c03240f40636bec9b` |
| [code-graph-rag](https://github.com/vitali87/code-graph-rag) | Vitali Avagyan | MIT | `bb5fde9817674dfdaee2480152872128876df04e` |
| [reverse-skill](https://github.com/zhaoxuya520/reverse-skill) | zhaoxuya520 | MIT | `7e2097fd90d25c2f976f6eba26d6c00aa88051df` |

The reverse-skill review retained general defensive concepts, not its code, prompts,
payloads or bootstrapper. See [the review record](docs/third-party/REVERSE_SKILL_REVIEW.md).
If future changes incorporate upstream code, preserve the relevant upstream copyright
and permission notices alongside it; acknowledgements alone are not a substitute.


# Release procedure

ad ships as a notarized macOS `.dmg`. CI handles the build automatically when you push a `v*` tag; locally you can run `pnpm release:mac`. This doc covers the one-time credential setup and the rotation playbook.

---

## One-time setup

### 1. Apple Developer credentials

You need:

- A **Developer ID Application** certificate exported as a `.p12` (Keychain Access → "Certificates" → right-click → Export). Note the export password.
- An **App Store Connect API key** (`AuthKey_<KEYID>.p8`) created at https://appstoreconnect.apple.com/access/integrations/api with **Developer** role.

### 2. GitHub secrets

In `Settings → Secrets and variables → Actions`, create:

| Secret                   | What it is                                                                    |
| ------------------------ | ----------------------------------------------------------------------------- |
| `APPLE_SIGNING_IDENTITY` | The full identity string, e.g. `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_CERT_BASE64`      | `base64 -i cert.p12` of the exported certificate                              |
| `APPLE_CERT_PASSWORD`    | The password you set when exporting the `.p12`                                |
| `KEYCHAIN_PASSWORD`      | Any random password — used only inside the runner's temp keychain             |
| `APPLE_API_KEY_ID`       | The 10-character Key ID shown next to the API key                             |
| `APPLE_API_ISSUER`       | The issuer UUID shown above the keys list                                     |
| `APPLE_API_KEY_BASE64`   | `base64 -i AuthKey_<KEYID>.p8`                                                |

> ⚠ **Never** commit `*.p8`, `*.p12`, or `.cer` files. The repo's `.gitignore` already blocks them.

### 3. Local environment (optional, for `pnpm release:mac` outside CI)

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
export APPLE_API_KEY_ID="ABCD1234EF"
export APPLE_API_ISSUER="12345678-1234-1234-1234-1234567890ab"
export APPLE_API_KEY_PATH="$HOME/.appstoreconnect/private_keys/AuthKey_ABCD1234EF.p8"
```

---

## Cutting a release

```bash
# bump version in package.json + src-tauri/Cargo.toml + tauri.conf.json
git commit -am "Release v0.1.0"
git tag v0.1.0
git push --tags
```

CI will build, sign, notarize, staple, and attach the DMG to a new GitHub Release.

For a local dry-run:

```bash
pnpm release:mac
```

---

## Rotation

- **API key**: revoke in App Store Connect, generate a new one, update `APPLE_API_KEY_ID` + `APPLE_API_KEY_BASE64` secrets.
- **Signing certificate**: when it expires (~1 year), generate a new CSR, request a new Developer ID Application cert, export, re-encode, update `APPLE_CERT_BASE64` + `APPLE_CERT_PASSWORD`.

---

## Smoke test (after every release)

On a Mac that has never seen ad:

```bash
spctl -a -vvv /Applications/ad.app
# Expected: accepted, source=Notarized Developer ID
```

If Gatekeeper still complains after notarization, check:

1. The certificate is `Developer ID Application` (not `Developer ID Installer` or `Mac App Distribution`).
2. The notarization ticket was stapled (`stapler validate`).
3. Hardened runtime is enabled (it is, via `entitlements.plist`).

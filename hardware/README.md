# hardware/: device packages

Each directory with a `debian/` is one Debian source package of hardware support for a particular machine, or a helper
those packages share. Every directory's own `README.md` says what the package changes and what is verified on real
hardware and what is not. Install a device package only on the machine it is for.

| Directory | Binary packages |
|---|---|
| `panasonic_cf19` | `panasonic-cf19-support`, `panasonic-hbtn-dkms` (Toughbook CF-19; the DKMS package also serves the Toughpad FZ-G1) |
| `panasonic_fz_g1` | `panasonic-fz-g1-support` |
| `hp_spectre_x360_16` | `hp-spectre-x360-16-support` |
| `hp_notebook_81ff` | `hp-notebook-81ff-support` |
| `lenovo_thinkpad_t430` | `lenovo-thinkpad-t430-support` |
| `lenovo_thinkpad_yoga_11e` | `lenovo-thinkpad-yoga-11e-support` |
| `lenovo_yoga_c940_14iil` | `lenovo-yoga-c940-14iil-support`, `lenovo-yoga-c940-14iil-windows-vm` |
| `grub_large_console` | `grub-large-console` |
| `convertible_helper`, `convertible_power` | helpers needed by the convertible and tablet packages |

## Installing from the hams.com APT repository

The packages are in the signed repository at `https://hams.com/relay/apt` (suite `stable`, component `main`, architectures
`amd64` and `arm64`; these packages are `Architecture: all`, so every architecture lists them), with source packages for
`deb-src`. Debian 13 (trixie) is the tested release.

Add the signing key as a keyring file (not `apt-key`). Its fingerprint is
`789C 1B1C 50D5 71CF 63DF BAF2 EEF1 5F6B 744B FFD6` (RSA 4096, expires 2028-09-19); check it before trusting the file,
because a key downloaded from the package server adds no trust by itself:

```
sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://hams.com/relay/apt/hams-relay-repo-signing.gpg -o /tmp/hams-relay-repo-signing.gpg
gpg --show-keys --with-colons --fingerprint /tmp/hams-relay-repo-signing.gpg | awk -F: '/^fpr:/{print $10; exit}'
sudo cp /tmp/hams-relay-repo-signing.gpg /etc/apt/keyrings/hams-relay.gpg
```

`/etc/apt/sources.list.d/hams.sources`:

```
Types: deb deb-src
URIs: https://hams.com/relay/apt
Suites: stable
Components: main
Signed-By: /etc/apt/keyrings/hams-relay.gpg
```

Then `sudo apt update && sudo apt install panasonic-cf19-support` (or the package for your machine). Use `Types: deb` to
leave out source packages; `apt source panasonic-cf19` fetches a source package. `add_apt_repo.sh` in the repository root
does the key step with the key embedded and the fingerprint pinned, and writes the relay's `hams-relay.list`; remove that
file if you add `hams.sources` (apt warns about a repository configured twice). The same text is `README.txt` in the
repository root.

## Building and publishing (maintainers)

`hardware/build_packages.sh <outdir> [dir...]` builds every package (binary and native 3.0 source) from a clean copy of the
files git tracks, so a file that is untracked or excluded by `.gitignore` (`*.patch` is) cannot hide a missing file. With
`LINTIAN=1` it also runs `lintian --pedantic` and fails on any tag. The output directory then holds `.deb`, `.dsc`,
`.tar.xz` and `SHA256SUMS`. Publishing is `.github/workflows/publish_apt_packages.sh <files...>` in the hams_com repository:
it merges the files into what the repository already holds, signs with the repository key and prunes nothing else. See
`docs/runbooks/RELAY_RELEASE.md` in hams_com. Tests: `python3 -m unittest hardware/test_build_packages.py`.

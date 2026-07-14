# Publishing g15-cli to the AUR

Frozen 2026-07-10: AUR user registration was disabled, so the first publish is
pending. Everything below is ready — `aur/PKGBUILD` + `aur/.SRCINFO` are
makepkg-tested against the `v0.1.0` tag.

## One-time setup (blocked on AUR registration reopening)

1. Register an account at <https://aur.archlinux.org/register>.
2. In *My Account*, add your SSH public key (`cat ~/.ssh/id_ed25519.pub`).

## First publish

```sh
git clone ssh://aur@aur.archlinux.org/g15-cli.git /tmp/aur-g15
cp aur/PKGBUILD aur/.SRCINFO /tmp/aur-g15/
cd /tmp/aur-g15
git add -A
git commit -m "g15-cli 0.1.0"
git push
```

Cloning a non-existent package name is how the AUR creates it — the empty
clone warning is expected. After the push, verify at
<https://aur.archlinux.org/packages/g15-cli> and test `yay -S g15-cli`.

## Future releases

```sh
# in the repo
vim Cargo.toml                      # bump version = "X.Y.Z"
cargo build                         # refreshes Cargo.lock
git commit -am "vX.Y.Z" && git tag vX.Y.Z && git push && git push origin vX.Y.Z

# update packaging
cd aur
curl -sLO https://github.com/andeen171/g15-cli/archive/vX.Y.Z.tar.gz
sha256sum vX.Y.Z.tar.gz             # put into PKGBUILD sha256sums
vim PKGBUILD                        # pkgver=X.Y.Z, pkgrel=1, new sha256
makepkg -f                          # must build clean
makepkg --printsrcinfo > .SRCINFO
rm -rf src pkg *.tar.* && git commit -am "aur: X.Y.Z" && git push

# push to the AUR remote
cp PKGBUILD .SRCINFO /tmp/aur-g15/
cd /tmp/aur-g15 && git commit -am "g15-cli X.Y.Z" && git push
```

## Notes

- `.SRCINFO` must be regenerated on every PKGBUILD change; the AUR rejects
  pushes where it's stale.
- Keep `makedepends=('cargo')` / `--frozen` builds; the AUR guidelines for
  Rust packages are followed in the current PKGBUILD.
- If another maintainer grabs the `g15-cli` name meanwhile, either request
  co-maintainership or publish as `g15-cli-git`.

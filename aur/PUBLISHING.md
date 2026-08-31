# Publishing g15-cli to the AUR

Published at <https://aur.archlinux.org/packages/g15-cli> (maintainer
`andeen`, first push 2026-07-13). Follow *Future releases* below for every
version after that; the one-time setup is done.

## One-time setup (done)

1. An account at <https://aur.archlinux.org/register>.
2. Your SSH public key added under *My Account* (`cat ~/.ssh/id_ed25519.pub`).
3. `git clone ssh://aur@aur.archlinux.org/g15-cli.git` — cloning a
   non-existent package name is how the AUR creates it, so the empty-clone
   warning on the first push was expected.

## Future releases

The AUR clone lives at `~/dev/aur-g15-cli` — a permanent checkout, not a `/tmp`
one, because a throwaway clone is how a release ends up built, committed and
never actually pushed. It fetches over HTTPS and pushes over SSH:

```
origin  https://aur.archlinux.org/g15-cli.git      (fetch)
origin  ssh://aur@aur.archlinux.org/g15-cli.git    (push)
```

```sh
# in this repo
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

# publish to the AUR
cp PKGBUILD .SRCINFO ~/dev/aur-g15-cli/
cd ~/dev/aur-g15-cli && git commit -am "g15-cli X.Y.Z" && git push
```

The push authenticates with `~/.ssh/id_ed25519`, which has a passphrase, so it
needs an unlocked agent:

```sh
eval "$(ssh-agent -s)" && ssh-add ~/.ssh/id_ed25519
```

## Notes

- `.SRCINFO` must be regenerated on every PKGBUILD change; the AUR rejects
  pushes where it's stale.
- Keep `makedepends=('cargo')` / `--frozen` builds; the AUR guidelines for
  Rust packages are followed in the current PKGBUILD.
- After pushing, confirm the AUR actually took it. Read cgit, not the RPC —
  the RPC sits behind a cache and served the old version for minutes after a
  successful 0.3.0 push, which looks exactly like a push that never landed:
  `curl -s 'https://aur.archlinux.org/cgit/aur.git/plain/PKGBUILD?h=g15-cli' | grep ^pkgver`
  The package page agrees with cgit immediately; the RPC catches up on its own.
- The `omarchy-g15` bar plugin needs the CLI at 0.3.0 or newer for
  `g15 status`, so an AUR version behind that leaves plugin users with a
  widget that reads nothing.

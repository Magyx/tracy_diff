pkgname=tracy-diff
pkgver=0.1.0
pkgrel=1
pkgdesc='Compare two Tracy profiler traces and report performance differences'
arch=('x86_64')
url='https://github.com/Magyx/tracy_diff'
license=('MIT')
depends=('tracy')
makedepends=('cargo')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/latest.tar.gz")
sha256sums=('d0a0a235a486cc3b8e26c5b4f207c1016dfe73c2d2908fedf15e9043aab6cf61')

prepare() {
  cd "tracy_diff-latest"
  export RUSTUP_TOOLCHAIN=stable
  cargo fetch --locked --target "$(rustc -vV | sed -n 's/host: //p')"
}

build() {
  cd "tracy_diff-latest"
  export RUSTUP_TOOLCHAIN=stable
  export CARGO_TARGET_DIR=target
  cargo build --frozen --release
}

package() {
  cd "tracy_diff-latest"
  install -Dm755 "target/release/tracy_diff" "$pkgdir/usr/bin/tracy-diff"
}

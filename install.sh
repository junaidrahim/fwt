#!/bin/sh
set -eu

fwt_source_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
fwt_install_dir=${FWT_INSTALL_DIR:-"${HOME}/.local/bin"}
fwt_install_prefix=$(dirname -- "${fwt_install_dir}")
fwt_man_dir=${FWT_MAN_DIR:-"${fwt_install_prefix}/share/man/man1"}

cargo build --release --bin git-fwt --manifest-path "${fwt_source_dir}/Cargo.toml"
mkdir -p "${fwt_install_dir}"
mkdir -p "${fwt_man_dir}"
install -m 755 "${fwt_source_dir}/target/release/git-fwt" "${fwt_install_dir}/git-fwt"
install -m 644 "${fwt_source_dir}/docs/git-fwt.1" "${fwt_man_dir}/git-fwt.1"
ln -sfn git-fwt "${fwt_install_dir}/fwt"

printf 'installed git-fwt and fwt -> git-fwt in %s\n' "${fwt_install_dir}"
printf 'installed git-fwt(1) in %s\n' "${fwt_man_dir}"

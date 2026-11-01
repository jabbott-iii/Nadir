#!/bin/sh
# Turns DIR, which holds a release's .deb packages, into the flat apt
# repository the release workflow publishes as release assets (ADR-0036):
#
#     Packages, Packages.gz   what each package is, and its file
#     Release                 the hashes of those, dated
#     InRelease, Release.gpg  Release signed with KEY, inline and apart
#     nadir-archive-keyring.gpg  KEY's public half, for `signed-by=`
#
# KEY names a secret key in gpg's keyring, which the workflow imports
# from the APT_SIGNING_KEY secret. Users add the repository with
#
#     deb [signed-by=/usr/share/keyrings/nadir-archive-keyring.gpg] \
#         https://github.com/jabbott-iii/Nadir/releases/latest/download ./
#
# and apt asks for `<that URL>/./InRelease` and `/./Packages.gz`, which
# GitHub serves, and for each package by its bare name.
#
# Usage: apt-repo.sh DIR KEY
set -eu

if [ "$#" -ne 2 ]; then
    echo "usage: apt-repo.sh DIR KEY" >&2
    exit 2
fi
dir=$1
key=$2
cd "$dir"
ls ./*.deb > /dev/null

# apt-ftparchive names each file `./name.deb`; apt joins the repository's
# URL and the name, and a release asset's URL has no `./` in it.
apt-ftparchive -o APT::FTPArchive::MD5=false -o APT::FTPArchive::SHA1=false packages . |
    sed 's|^Filename: \./|Filename: |' > Packages
gzip -9 -n -k -f Packages

# Written apart and moved in: `release .` hashes every index file it finds,
# and would hash a half-written Release in its own list.
release=$(mktemp)
apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=Nadir \
    -o APT::FTPArchive::Release::Label=Nadir \
    -o APT::FTPArchive::Release::Architectures="amd64 arm64" \
    -o APT::FTPArchive::Release::Description="Nadir game engine: nadir-cook and nadir-sandbox" \
    -o APT::FTPArchive::MD5=false \
    -o APT::FTPArchive::SHA1=false \
    release . > "$release"
mv "$release" Release
chmod 0644 Release

gpg --batch --yes --local-user "$key" --digest-algo SHA512 --clearsign --output InRelease Release
gpg --batch --yes --local-user "$key" --digest-algo SHA512 --armor --detach-sign --output Release.gpg Release
gpg --batch --yes --export "$key" > nadir-archive-keyring.gpg

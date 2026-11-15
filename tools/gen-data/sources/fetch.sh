#!/bin/sh
set -eu

destination=${1:-p4in}
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

mkdir -p "$destination/aozora/zips" "$destination/gutenberg" "$destination/scowl" "$destination/tatoeba" "$destination/shinonome"
cp "$script_dir/aozora_selection.tsv" "$destination/aozora/selection.tsv"

fetch() {
    url=$1
    output=$2
    if [ -s "$output" ]; then
        return
    fi
    temporary="${output}.download"
    curl --fail --location --output "$temporary" "$url"
    mv "$temporary" "$output"
    sleep 2
}

while IFS="$(printf '\t')" read -r id author title input_by url; do
    fetch "$url" "$destination/aozora/zips/${id}.zip"
done < "$script_dir/aozora_selection.tsv"

while IFS="$(printf '\t')" read -r id title author; do
    fetch "https://www.gutenberg.org/cache/epub/${id}/pg${id}.txt" "$destination/gutenberg/pg${id}.txt"
done < "$script_dir/public_domain_books.tsv"

scowl_archive="$destination/scowl/scowl-2020.12.07.tar.gz"
fetch "https://downloads.sourceforge.net/wordlist/scowl-2020.12.07.tar.gz" "$scowl_archive"
if [ ! -d "$destination/scowl/scowl-2020.12.07" ]; then
    tar -xzf "$scowl_archive" -C "$destination/scowl"
fi

fetch "https://downloads.tatoeba.org/exports/per_language/eng/eng_sentences_CC0.tsv.bz2" \
    "$destination/tatoeba/eng_sentences_CC0.tsv.bz2"

shinonome_archive="$destination/shinonome/shinonome-0.9.11p1.tar.bz2"
shinonome_sha256=95663c95c92ba5765f63ccbdf033eb93b707be01812a989c548db943479c838f
fetch "http://openlab.ring.gr.jp/efont/dist/shinonome/shinonome-0.9.11p1.tar.bz2" \
    "$shinonome_archive"
actual_sha256=$(shasum -a 256 "$shinonome_archive" | awk '{print $1}')
if [ "$actual_sha256" != "$shinonome_sha256" ]; then
    echo "Shinonome SHA-256 mismatch: expected $shinonome_sha256, got $actual_sha256" >&2
    exit 1
fi
if [ ! -d "$destination/shinonome/shinonome-0.9.11" ]; then
    tar -xjf "$shinonome_archive" -C "$destination/shinonome"
fi

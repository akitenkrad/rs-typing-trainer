#!/bin/sh
set -eu

destination=${1:-p4in}
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

mkdir -p "$destination/aozora/zips" "$destination/gutenberg" "$destination/scowl" "$destination/tatoeba"
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

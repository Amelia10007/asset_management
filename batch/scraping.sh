#!/bin/bash

SCRIPT_DIR=$(cd $(dirname $0); pwd)
CHECK_FILE="../log/scraping_now.tmp.log"

pushd $SCRIPT_DIR

if [-e $CHECK_FILE]; then
    echo 'Previous batch not finished yet' >> ../log/log.log
fi

echo 'Batch is running. Do not delete this file manually.' > $CHECK_FILE

started=$(date)
echo "scraping batch started at ${started}" >> ../log/log.log

pushd ../rust/nicehash_scraper
../target/debug/nicehash_scraper >> ../../log/scraper.log
popd

pushd ../rust/nicehash_speculator
../target/debug/nicehash_speculator >> ../../log/speculator.log
popd

finished=$(date)
echo "scraping batch finished at ${finished}" >> ../log/log.log

rm $CHECK_FILE

popd

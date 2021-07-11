#!/bin/bash

SCRIPT_DIR=$(cd $(dirname $0); pwd)

pushd $SCRIPT_DIR

started=$(date)
echo "scraping batch started at ${started}" >> ../log/log.log

pushd ../rust/nicehash_scraper
../target/debug/nicehash_scraper >> ../../log/scraper.log
popd

finished=$(date)
echo "scraping batch finished at ${finished}" >> ../log/log.log

popd
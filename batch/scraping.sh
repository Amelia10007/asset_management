#!/bin/bash

SCRIPT_DIR=$(cd $(dirname $0); pwd)
LOG_DIR=$SCRIPT_DIR/../log

pushd $SCRIPT_DIR

started=$(date)
echo "scraping batch started at ${started}" >> $LOG_DIR/log.log

pushd ../rust/nicehash_scraper
../target/release/nicehash_scraper >> $LOG_DIR/scraper.log 2>&1
popd

pushd ../rust/nicehash_speculator
../target/release/nicehash_speculator >> $LOG_DIR/speculator.log 2>&1
popd

finished=$(date)
echo "scraping batch finished at ${finished}" >> $LOG_DIR/log.log

popd

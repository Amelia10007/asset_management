#!/bin/bash

SCRIPT_DIR=$(cd $(dirname $0); pwd)
LOG_DIR=$SCRIPT_DIR/../log
CHECK_FILE=$LOG_DIR/scraping_now.tmp.log

pushd $SCRIPT_DIR

if [-e $CHECK_FILE]; then
    echo 'Previous batch not finished yet' >> $LOG_DIR/log.log
fi

echo 'Batch is running. Do not delete this file manually.' > $CHECK_FILE

started=$(date)
echo "scraping batch started at ${started}" >> $LOG_DIR/log.log

pushd ../rust/nicehash_scraper
../target/debug/nicehash_scraper >> $LOG_DIR/scraper.log 2>&1
popd

pushd ../rust/nicehash_speculator
../target/debug/nicehash_speculator >> $LOG_DIR/speculator.log 2>&1
popd

finished=$(date)
echo "scraping batch finished at ${finished}" >> $LOG_DIR/log.log

rm $CHECK_FILE

popd

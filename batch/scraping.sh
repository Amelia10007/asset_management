#!/bin/bash

SCRIPT_DIR=$(cd $(dirname $0); pwd)
LOG_DIR=$SCRIPT_DIR/../log

pushd $SCRIPT_DIR

started=$(date)
echo "scraping batch started at ${started}" >> $LOG_DIR/log.log

pushd ../rust/nicehash_scraper
readonly NICEHASH_DB_URL='mysql://autotrader:autotrader@127.0.0.1:3307/trade'
readonly NICEHASH_SIMULATION_DB_URL='mysql://autotrader:autotrader@127.0.0.1:3307/trade_simulation'
# Update DB using data from nicehash
sed -i -e "s|^DATABASE_URL=.*|DATABASE_URL=$NICEHASH_DB_URL|" .env
../target/release/nicehash_scraper >> $LOG_DIR/scraper.log 2>&1
# Update simulation DB
sed -i -e "s|^DATABASE_URL=.*|DATABASE_URL=$NICEHASH_SIMULATION_DB_URL|" .env
../target/release/nicehash_scraper >> $LOG_DIR/scraper.log 2>&1
# Reset DB URL
sed -i -e "s|^DATABASE_URL=.*|DATABASE_URL=$NICEHASH_DB_URL|" .env
popd

pushd ../rust/nicehash_speculator
../target/release/nicehash_speculator >> $LOG_DIR/speculator.log 2>&1
popd

finished=$(date)
echo "scraping batch finished at ${finished}" >> $LOG_DIR/log.log

popd

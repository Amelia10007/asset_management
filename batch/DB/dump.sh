#!/bin/bash

echo 'asset_management dump started'

# Destination
SCRIPT_DIR=$(cd $(dirname $0); pwd)
NOW=$(date +%Y%m%d)
DESTINATION_DIR=$SCRIPT_DIR/dump
DESTINATION=$SCRIPT_DIR/dump/dump_$NOW.sql

mkdir -p $DESTINATION_DIR

echo "Dump destination: $DESTINATION"

# Dump
CONFIG=$SCRIPT_DIR/mysql.cnf
TARGET_DB='trade'
mysqldump --defaults-extra-file=$CONFIG $TARGET_DB > $DESTINATION

echo 'asset_management dump finished'

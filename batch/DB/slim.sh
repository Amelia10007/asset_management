#!/bin/bash

echo 'asset_management DB-slim started'

SCRIPT_DIR=$(cd $(dirname $0); pwd)

MYSQL_SCRIPT=$SCRIPT_DIR/slim.sql
CONFIG=$SCRIPT_DIR/mysql.cnf

mysql --defaults-extra-file=$CONFIG < $MYSQL_SCRIPT

echo 'asset_management DB-slim finished'

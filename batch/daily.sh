#!/bin/bash

SCRIPT_DIR=$(cd $(dirname $0); pwd)

pushd $SCRIPT_DIR

started=$(date)

echo "daily.sh started now: ${started}" >> ../log/log.log
echo '' >> ../log/log.log

# docker container ls -a
# docker start container_id
# docker exec -i -t container_id /bin/bash
# service mysql start

./batch_nicehash ../nicehash/api_key >> ../log/log.log
./batch_coincheck ../coincheck/api_key >> ../log/log.log

finished=$(date)

echo "daily.sh finished now: ${finished}" >> ../log/log.log
echo '' >> ../log/log.log

popd
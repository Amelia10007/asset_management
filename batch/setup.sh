# NOTE: Some commands may need super user previledge

SCRIPT_DIR=$(cd $(dirname $0); pwd)

pushd $SCRIPT_DIR

# Build
pushd ../rust
cargo build
popd

# Start DB
pushd ../docker-autotrader-db
docker-compose up
popd

# Setup cron
crontab -l > tmpcron
echo >> tmpcron
echo '# Automatically appended schedule by asset_management' >> tmpcron
echo "*/5 * * * * ${SCRIPT_DIR}/scraping.sh" >> tmpcron
crontab tmpcron

service cron enable
service cron restart

popd
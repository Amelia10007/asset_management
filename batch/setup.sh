SCRIPT_DIR=$(cd $(dirname $0); pwd)

pushd $SCRIPT_DIR

pushd ../rust

cargo build

popd

popd
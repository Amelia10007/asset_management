SCRIPT_DIR=$(cd $(dirname $0); pwd)

LOG_DIR=$SCRIPT_DIR/../log
LOG_ARCHIVE=$LOG_DIR/archive.tar.gz
BATCH_LOG=$LOG_DIR/archive.log

rm $LOG_ARCHIVE
tar -zcvf $LOG_ARCHIVE $LOG_DIR

for LOG_FILE in $LOG_DIR/*.log; do
    echo > $LOG_FILE
done

echo "Log archived at `date`" >> $BATCH_LOG

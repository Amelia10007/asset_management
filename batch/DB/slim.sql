use trade;

-- Delete empty balance records
-- DELETE FROM balance WHERE available + pending = 0;

-- Delete old orderbooks, which are not used to asset management
DELETE orderbook
FROM orderbook
INNER JOIN stamp ON stamp.stamp_id = orderbook.stamp_id
WHERE stamp.stamp < DATE_SUB(CURDATE(), INTERVAL 1 DAY);

-- Print DB size after slim
SELECT table_schema, sum(data_length) /1024/1024 AS SizeMB
FROM information_schema.tables
GROUP BY table_schema
HAVING table_schema = 'trade'
ORDER BY sum(data_length+index_length) DESC;

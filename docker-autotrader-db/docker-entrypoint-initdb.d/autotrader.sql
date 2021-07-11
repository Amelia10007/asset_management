DROP DATABASE IF EXISTS trade;

CREATE DATABASE trade;

use trade;

-- **********table relationship**********
--
--          2      1
-- currency-----------market-----price
--  |                 |   |_______   |____
--  |                 |           |       |
-- balance        orderbook    myorder    |
--      |               |           |     |
--      -----stamp-------------------------
--
-- next_id

CREATE TABLE currency
(
    currency_id INTEGER NOT NULL PRIMARY KEY,
    -- currency unit, ex. BTC, ETH, ...
    symbol VARCHAR(8) NOT NULL,
    -- ex. Bitcoin, Ether, ...
    name VARCHAR(32) NOT NULL
);

CREATE TABLE stamp
(
    stamp_id INTEGER NOT NULL PRIMARY KEY,
    stamp TIMESTAMP NOT NULL
);

CREATE TABLE balance
(
    balance_id INTEGER NOT NULL PRIMARY KEY,
    currency_id INTEGER NOT NULL,
    stamp_id INTEGER NOT NULL,
    balance FLOAT NOT NULL,

    FOREIGN KEY (currency_id) REFERENCES currency(currency_id) ON UPDATE CASCADE,
    FOREIGN KEY (stamp_id) REFERENCES stamp(stamp_id) ON UPDATE CASCADE
);

CREATE TABLE market
(
    market_id INTEGER NOT NULL PRIMARY KEY,
    base_id INTEGER NOT NULL,
    quote_id INTEGER NOT NULL,

    FOREIGN KEY (base_id) REFERENCES currency(currency_id) ON UPDATE CASCADE,
    FOREIGN KEY (quote_id) REFERENCES currency(currency_id) ON UPDATE CASCADE
);

CREATE TABLE price
(
    price_id INTEGER NOT NULL PRIMARY KEY,
    market_id INTEGER NOT NULL,
    stamp_id INTEGER NOT NULL,
    price FLOAT NOT NULL,

    FOREIGN KEY (market_id) REFERENCES market(market_id) ON UPDATE CASCADE,
    FOREIGN KEY (stamp_id) REFERENCES stamp(stamp_id) ON UPDATE CASCADE
);

CREATE TABLE orderbook
(
    orderbook_id INTEGER NOT NULL PRIMARY KEY,
    market_id INTEGER NOT NULL,
    stamp_id INTEGER NOT NULL,
    is_buy BOOLEAN NOT NULL,
    price FLOAT NOT NULL,
    volume FLOAT NOT NULL,

    FOREIGN KEY (market_id) REFERENCES market(market_id) ON UPDATE CASCADE,
    FOREIGN KEY (stamp_id) REFERENCES stamp(stamp_id) ON UPDATE CASCADE
);

CREATE TABLE myorder
(
    myorder_id INTEGER NOT NULL PRIMARY KEY,
    -- order id on remote trading service
    transaction_id VARCHAR(64) NOT NULL,
    market_id INTEGER NOT NULL,
    created_stamp_id INTEGER NOT NULL,
    modified_stamp_id INTEGER NOT NULL,
    price FLOAT NOT NULL,
    base_quantity FLOAT NOT NULL,
    quote_quantity FLOAT NOT NULL,
    state VARCHAR(32) NOT NULL,

    FOREIGN KEY (market_id) REFERENCES market(market_id) ON UPDATE CASCADE,
    FOREIGN KEY (created_stamp_id) REFERENCES stamp(stamp_id) ON UPDATE CASCADE,
    FOREIGN KEY (modified_stamp_id) REFERENCES stamp(stamp_id) ON UPDATE CASCADE
);

CREATE TABLE next_id
(
    currency INTEGER NOT NULL,
    stamp INTEGER NOT NULL,
    balance INTEGER NOT NULL,
    market INTEGER NOT NULL,
    price INTEGER NOT NULL,
    orderbook INTEGER NOT NULL,
    myorder INTEGER NOT NULL
);

-- First ids
INSERT INTO next_id VALUES (0, 0, 0, 0, 0, 0, 0);

DROP USER IF EXISTS autotrader;
CREATE USER IF NOT EXISTS autotrader IDENTIFIED BY 'autotrader';
GRANT SELECT, INSERT, UPDATE, DELETE ON trade.* TO autotrader;

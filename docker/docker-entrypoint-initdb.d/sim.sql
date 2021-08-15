DROP DATABASE IF EXISTS sim;

CREATE DATABASE sim;

use sim;

CREATE TABLE balance
(
    balance_id INTEGER NOT NULL PRIMARY KEY,
    currency_id INTEGER NOT NULL,
    stamp_id INTEGER NOT NULL,
    available FLOAT NOT NULL,
    pending FLOAT NOT NULL
);

GRANT SELECT, INSERT, UPDATE, DELETE ON sim.* TO autotrader;

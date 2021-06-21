DROP DATABASE IF EXISTS asset;

CREATE DATABASE asset;

use asset;

CREATE TABLE service
(
    service_id INTEGER NOT NULL,
    name VARCHAR(32) NOT NULL,

    PRIMARY KEY (service_id)
);

CREATE TABLE asset
(
    asset_id INTEGER NOT NULL,
    name VARCHAR(32),
    unit VARCHAR(16),

    PRIMARY KEY (asset_id)
);

CREATE TABLE date
(
    date DATE NOT NULL,

    PRIMARY KEY (date)
);

CREATE TABLE exchange
(
    exchange_id INTEGER NOT NULL,
    date DATE NOT NULL,
    base_asset_id INTEGER NOT NULL,
    target_asset_id INTEGER NOT NULL,
    rate FLOAT NOT NULL,

    PRIMARY KEY (exchange_id),

    FOREIGN KEY (date) REFERENCES date(date),
    FOREIGN KEY (base_asset_id) REFERENCES asset(asset_id) ON UPDATE CASCADE,
    FOREIGN KEY (target_asset_id) REFERENCES asset(asset_id) ON UPDATE CASCADE
);

CREATE TABLE history
(
    history_id INTEGER NOT NULL,
    date DATE NOT NULL,
    service_id INTEGER NOT NULL,
    asset_id INTEGER NOT NULL,
    amount FLOAT NOT NULL,

    PRIMARY KEY (history_id),

    FOREIGN KEY (date) REFERENCES date(date),
    FOREIGN KEY (service_id) REFERENCES service(service_id) ON UPDATE CASCADE,
    FOREIGN KEY (asset_id) REFERENCES asset(asset_id) ON UPDATE CASCADE
);

CREATE TABLE next_id
(
    service_id INTEGER NOT NULL,
    asset_id INTEGER NOT NULL,
    exchange_id INTEGER NOT NULL,
    history_id INTEGER NOT NULL
);

-- First ids
INSERT INTO next_id VALUES (0, 0, 0, 0);

-- User for batch process
DROP USER IF EXISTS asset_management_batch;
CREATE USER IF NOT EXISTS asset_management_batch IDENTIFIED BY 'asset_management_batch';
GRANT SELECT, INSERT, UPDATE ON asset.* TO asset_management_batch;

-- User for analytics application
DROP USER IF EXISTS asset_management_app;
CREATE USER IF NOT EXISTS asset_management_app IDENTIFIED BY 'asset_management_app';
GRANT SELECT ON asset.* TO asset_management_app;

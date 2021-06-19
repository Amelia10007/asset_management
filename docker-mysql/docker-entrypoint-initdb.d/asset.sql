DROP DATABASE IF EXISTS asset;

CREATE DATABASE asset;

use asset;

CREATE TABLE service
(service_name VARCHAR(32) NOT NULL,

PRIMARY KEY (service_name)
);

CREATE TABLE asset
(asset_name VARCHAR(32) NOT NULL,

PRIMARY KEY (asset_name)
);

CREATE TABLE date
(
date DATE NOT NULL,

PRIMARY KEY (date)
);

CREATE TABLE exchange
(date DATE NOT NULL,
base_asset_name VARCHAR(32) NOT NULL,
target_asset_name VARCHAR(32) NOT NULL,
rate FLOAT NOT NULL,

PRIMARY KEY (date, base_asset_name, target_asset_name),

FOREIGN KEY (date) REFERENCES date(date),
FOREIGN KEY (base_asset_name)
    REFERENCES asset(asset_name)
    ON UPDATE CASCADE,
FOREIGN KEY (target_asset_name)
    REFERENCES asset(asset_name)
    ON UPDATE CASCADE
);

CREATE TABLE history
(date DATE NOT NULL,
service_name VARCHAR(32) NOT NULL,
asset_name VARCHAR(32) NOT NULL,
amount FLOAT NOT NULL,

PRIMARY KEY (date, service_name, asset_name),

FOREIGN KEY (date) REFERENCES date(date),
FOREIGN KEY (service_name)
    REFERENCES service(service_name)
    ON UPDATE CASCADE,
FOREIGN KEY (asset_name)
    REFERENCES asset(asset_name)
    ON UPDATE CASCADE
);

-- User for batch process
CREATE USER IF NOT EXISTS batch IDENTIFIED BY 'batch';
GRANT SELECT, INSERT, UPDATE ON asset.* TO batch;

-- User for analytics application
CREATE USER IF NOT EXISTS app IDENTIFIED BY 'app';
GRANT SELECT ON asset.* TO app;

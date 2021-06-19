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
(id INTEGER AUTO_INCREMENT,
date DATE NOT NULL,
base_asset_name VARCHAR(32) NOT NULL,
target_asset_name VARCHAR(32) NOT NULL,
rate DECIMAL NOT NULL,

PRIMARY KEY (id),
INDEX (id),

FOREIGN KEY (base_asset_name)
    REFERENCES asset(asset_name)
    ON UPDATE CASCADE,

FOREIGN KEY (target_asset_name)
    REFERENCES asset(asset_name)
    ON UPDATE CASCADE
);

CREATE TABLE history
(id INTEGER AUTO_INCREMENT,
date DATE NOT NULL,
service_name VARCHAR(32) NOT NULL,
asset_name VARCHAR(32) NOT NULL,
amount DECIMAL NOT NULL,

PRIMARY KEY (id),
INDEX (id),

FOREIGN KEY (service_name)
    REFERENCES service(service_name)
    ON UPDATE CASCADE,

FOREIGN KEY (asset_name)
    REFERENCES asset(asset_name)
    ON UPDATE CASCADE
);

-- User for batch process
CREATE USER IF NOT EXISTS batch IDENTIFIED BY 'batch';
GRANT SELECT, INSERT ON asset.* TO batch;

-- User for analytics application
CREATE USER IF NOT EXISTS app IDENTIFIED BY 'app';
GRANT SELECT ON asset.* TO app;

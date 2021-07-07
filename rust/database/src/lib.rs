use common::alias::Result;
use common::err::{ErrSuger, OkOpt};
pub use mysql;
use mysql::prelude::*;
pub use mysql::time::Date;
use mysql::TxOpts;
use mysql::{Conn, Transaction};

pub mod entity;

pub use entity::*;

pub fn connect_asset_database_as_batch() -> Result<Conn> {
    Conn::new("mysql://asset_management_batch:asset_management_batch@localhost:3306/asset")
        .map_err(Into::into)
}

pub fn connect_asset_database_as_app() -> Result<Conn> {
    Conn::new("mysql://asset_management_app:asset_management_app@localhost:3306/asset")
        .map_err(Into::into)
}

pub trait AssetDatabase: Queryable {
    fn start_transaction(&mut self) -> Result<Transaction<'_>>;

    fn service_by_name(&mut self, name: &str) -> Result<Option<Service>> {
        self.exec_first(
            "SELECT service_id, name FROM service WHERE name=?",
            vec![name],
        )
        .map(|opt| opt.map(|(id, name)| Service::new(ServiceId(id), name)))
        .map_err(Into::into)
    }

    fn asset_by_unit(&mut self, asset_unit: &str) -> Result<Option<Asset>> {
        self.exec_first(
            "SELECT asset_id, name, unit FROM asset WHERE unit=?",
            vec![asset_unit],
        )
        .map(|opt| opt.map(|(id, name, unit)| Asset::new(AssetId(id), name, unit)))
        .map_err(Into::into)
    }

    fn exchanges_by_date(&mut self, date: Date) -> Result<Vec<Exchange>> {
        self.exec_map(
            "SELECT base.asset_id, base.name, base.unit, target.asset_id, target.name, target.unit,  rate
            FROM exchange
            INNER JOIN asset AS base ON exchange.base_asset_id=base.asset_id
            INNER JOIN asset AS target ON exchange.target_asset_id=target.asset_id
            WHERE date=?",
            vec![date],
            |(base_id, base_name, base_unit, target_id, target_name, target_unit, rate)| Exchange {
                base: Asset::new(AssetId(base_id), base_name, base_unit),
                target: Asset::new(AssetId(target_id), target_name, target_unit),
                date,
                rate: Amount::new(rate),
            },
        ).map_err(Into::into)
    }

    fn histories_by_date(&mut self, date: Date) -> Result<Vec<History>> {
        self.exec_map(
            "SELECT service.service_id, service.name, asset.asset_id, asset.name, asset.unit, date, amount
            FROM history
            INNER JOIN service ON history.service_id=service.service_id
            INNER JOIN asset ON history.asset_id=asset.asset_id
            WHERE date=?",
            vec![date],
            |(service_id,service_name, asset_id,asset_name, unit, date, amount)| History {
                service: Service::new(ServiceId(service_id),service_name),
                asset: Asset::new(AssetId(asset_id),asset_name, unit),
                date,
                amount: Amount::new(amount),
            },
        ).map_err(Into::into)
    }

    fn insert_service(&mut self, service_name: &str) -> Result<Service> {
        if let Some(_) = self.exec_first::<i32, _, _>(
            "SELECT service_id FROM service WHERE name=?",
            vec![service_name],
        )? {
            ErrSuger::err_from(format!("Service {} is already exists", service_name))?;
        }

        let next_id: i32 = self
            .query_first("SELECT service_id FROM next_id")?
            .ok_opt("service_id undefined")?;

        let mut tx = self.start_transaction()?;

        tx.query_drop("UPDATE next_id SET service_id=service_id+1")?;

        tx.exec_drop(
            "INSERT INTO service (service_id, name) VALUES (?, ?)",
            (next_id, service_name),
        )?;

        tx.commit()?;

        Ok(Service::new(ServiceId(next_id), service_name.to_string()))
    }

    fn insert_asset(&mut self, name: Option<&str>, unit: Option<&str>) -> Result<Asset> {
        name.or(unit)
            .ok_opt("name or unit must be defined to register new asset.")?;

        if let Some(_) = self.exec_first::<i32, _, _>(
            "SELECT asset_id FROM asset WHERE name=? OR unit=?",
            (name, unit),
        )? {
            ErrSuger::err_from(format!(
                "Duplicate asset. name: {:?}, unit: {:?}",
                name, unit
            ))?;
        }

        let next_id: i32 = self
            .query_first("SELECT asset_id FROM next_id")?
            .ok_opt("asset_id undefined")?;

        let mut tx = self.start_transaction()?;

        tx.query_drop("UPDATE next_id SET asset_id=asset_id+1")?;

        tx.exec_drop(
            "INSERT INTO asset (asset_id, name, unit) VALUES (?, ?, ?)",
            (next_id, name, unit),
        )?;

        tx.commit()?;

        Ok(Asset::new(
            AssetId(next_id),
            name.map(ToOwned::to_owned),
            unit.map(ToOwned::to_owned),
        ))
    }

    fn insert_date(&mut self, date: Date) -> Result<Date> {
        self.exec_drop("INSERT INTO date (date) VALUES (?)", vec![date])?;
        Ok(date)
    }

    fn insert_exchange(
        &mut self,
        date: Date,
        base_asset_id: AssetId,
        target_asset_id: AssetId,
        rate: Amount,
    ) -> Result<()> {
        // Prevent duplicate recording
        if let Some(_) = self.exec_first::<i32, _, _>(
            "SELECT exchange_id FROM exchange
            WHERE date=? AND base_asset_id=? AND target_asset_id=?",
            (date, base_asset_id.0, target_asset_id.0),
        )? {
            ErrSuger::err_from("Duplicate exchange")?;
        }

        let next_id: i32 = self
            .query_first("SELECT exchange_id FROM next_id")?
            .ok_opt("exchange_id undefined")?;

        let mut tx = self.start_transaction()?;

        tx.query_drop("UPDATE next_id SET exchange_id=exchange_id+1")?;

        tx.exec_drop(
            "INSERT INTO exchange (exchange_id, date, base_asset_id, target_asset_id, rate)
            VALUES (?, ?, ?, ?, ?)",
            (
                next_id,
                date,
                base_asset_id.0,
                target_asset_id.0,
                rate.amount,
            ),
        )?;

        tx.commit()?;

        Ok(())
    }

    fn insert_hisotry(
        &mut self,
        service_id: ServiceId,
        asset_id: AssetId,
        date: Date,
        amount: Amount,
    ) -> Result<()> {
        // Prevent duplicate recording
        if let Some(_) = self.exec_first::<i32, _, _>(
            "SELECT history_id FROM history
            WHERE date=? AND service_id=? AND asset_id=?",
            (date, service_id.0, asset_id.0),
        )? {
            ErrSuger::err_from("Duplicate history")?;
        }

        let next_id: i32 = self
            .query_first("SELECT history_id FROM next_id")?
            .ok_opt("history_id undefined")?;

        let mut tx = self.start_transaction()?;

        tx.query_drop("UPDATE next_id SET history_id=history_id+1")?;

        tx.exec_drop(
            "INSERT INTO history (history_id, date, service_id, asset_id, amount)
            VALUES (?, ?, ?, ?, ?)",
            (next_id, date, service_id.0, asset_id.0, amount.amount),
        )?;

        tx.commit()?;

        Ok(())
    }
}

impl AssetDatabase for Conn {
    fn start_transaction(&mut self) -> Result<Transaction<'_>> {
        self.start_transaction(TxOpts::default())
            .map_err(Into::into)
    }
}

// $ cargo test -- --test-threads=1 is required for avoiding deadlock between transactions
// You should make all tables empty before tesing.
// You must restore all tables after testing.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connect_as_batch() {
        connect_asset_database_as_batch().unwrap();
    }

    #[test]
    fn test_connect_as_app() {
        connect_asset_database_as_app().unwrap();
    }

    #[test]
    fn test_histories_by_date() {
        let mut conn = connect_asset_database_as_batch().unwrap();

        let date = Date::try_from_ymd(2000, 1, 1).unwrap();
        let _histories = conn.histories_by_date(date).unwrap();
    }

    #[test]
    fn test_exchanges_by_date() {
        let mut conn = connect_asset_database_as_batch().unwrap();

        let date = Date::try_from_ymd(2000, 1, 1).unwrap();
        let _exchanges = conn.exchanges_by_date(date).unwrap();
    }

    #[test]
    #[ignore]
    fn test_insert_service() {
        let mut conn = connect_asset_database_as_batch().unwrap();

        let service_name = "suspicious_service";

        conn.insert_service(service_name).unwrap();

        let service = conn.service_by_name(service_name);
        assert!(matches!(service, Ok(Some(_))));
        assert_eq!(service_name, service.unwrap().unwrap().name);

        // Cannot insert duplicate record
        assert!(conn.insert_service(service_name).is_err());

        // Other record is OK
        conn.insert_service("alternative-serve").unwrap();
    }

    #[test]
    #[ignore]
    fn test_insert_asset() {
        let mut conn = connect_asset_database_as_batch().unwrap();

        let name = Some("suspicious_asset");
        let unit = Some("wowwowwfoo");

        conn.insert_asset(name, unit).unwrap();

        let asset = conn.asset_by_unit(unit.unwrap());
        assert!(matches!(asset, Ok(Some(_))));
        assert_eq!(name, asset.unwrap().unwrap().name.as_deref());

        // Cannot insert duplicate record
        assert!(conn.insert_asset(name, unit).is_err());

        // Other record is OK
        conn.insert_asset(Some("alter"), None).unwrap();
        conn.insert_asset(None, Some("ALT")).unwrap();
        assert!(conn.insert_asset(None, None).is_err());
    }

    #[test]
    #[ignore]
    fn test_insert_date() {
        let mut conn = connect_asset_database_as_batch().unwrap();

        let date = Date::try_from_ymd(2000, 1, 1).unwrap();

        conn.insert_date(date).unwrap();

        // Cannot insert duplicate record
        assert!(conn.insert_date(date).is_err());

        // Other record is OK
        conn.insert_date(date.next_day()).unwrap();
    }

    #[test]
    #[ignore]
    fn test_insert_exchange() {
        let mut conn = connect_asset_database_as_batch().unwrap();

        let date = Date::try_from_ymd(1998, 1, 1).unwrap();
        let base_name = "base".into();
        let base_unit = "BSE".into();
        let target_name = "target".into();
        let target_unit = "TGT".into();

        // Prepare
        conn.insert_date(date).unwrap();
        conn.insert_asset(base_name, base_unit).unwrap();
        conn.insert_asset(target_name, target_unit).unwrap();

        let base_id = conn.asset_by_unit(base_unit.unwrap()).unwrap().unwrap().id;
        let target_id = conn
            .asset_by_unit(target_unit.unwrap())
            .unwrap()
            .unwrap()
            .id;

        let rate = Amount::new(100.0);

        // New exchange record
        conn.insert_exchange(date, base_id, target_id, rate)
            .unwrap();

        // Verify
        let exchange = conn
            .exchanges_by_date(date)
            .unwrap()
            .into_iter()
            .find(|e| e.date == date)
            .unwrap();

        assert_eq!(base_id, exchange.base.id);
        assert_eq!(base_name, exchange.base.name.as_deref());
        assert_eq!(base_unit, exchange.base.unit.as_deref());

        assert_eq!(target_id, exchange.target.id);
        assert_eq!(target_name, exchange.target.name.as_deref());
        assert_eq!(target_unit, exchange.target.unit.as_deref());

        assert_eq!(Amount::new(100.0), exchange.rate);

        // Cannot insert duplicate record
        assert!(conn
            .insert_exchange(date, base_id, target_id, Amount::new(200.0))
            .is_err());

        // Other record is OK
        conn.insert_date(date.next_day()).unwrap();
        conn.insert_exchange(date.next_day(), base_id, target_id, Amount::new(200.0))
            .unwrap();
    }

    #[test]
    #[ignore]
    fn test_insert_history() {
        let mut conn = connect_asset_database_as_batch().unwrap();

        let date = Date::try_from_ymd(1999, 1, 1).unwrap();

        let service_name = "hololive";
        let asset_name = "bitcoin".into();
        let asset_unit = "BTC".into();

        // Prepare
        conn.insert_date(date).unwrap();
        conn.insert_service(service_name).unwrap();
        conn.insert_asset(asset_name, asset_unit).unwrap();

        let service_id = conn.service_by_name(service_name).unwrap().unwrap().id;
        let asset_id = conn.asset_by_unit(asset_unit.unwrap()).unwrap().unwrap().id;

        let amount = Amount::new(100.0);

        // New record
        conn.insert_hisotry(service_id, asset_id, date, amount)
            .unwrap();

        // Verify
        let history = conn
            .histories_by_date(date)
            .unwrap()
            .into_iter()
            .find(|h| h.date == date)
            .unwrap();

        assert_eq!(service_id, history.service.id);
        assert_eq!(service_name, history.service.name);

        assert_eq!(asset_id, history.asset.id);
        assert_eq!(asset_name, history.asset.name.as_deref());
        assert_eq!(asset_unit, history.asset.unit.as_deref());

        assert_eq!(date, history.date);
        assert_eq!(amount, history.amount);

        // Cannot insert duplicate record
        assert!(conn
            .insert_hisotry(service_id, asset_id, date, Amount::new(200.0))
            .is_err());

        // Other record is OK
        conn.insert_date(date.next_day()).unwrap();
        conn.insert_hisotry(service_id, asset_id, date.next_day(), Amount::new(200.0))
            .unwrap();
    }
}

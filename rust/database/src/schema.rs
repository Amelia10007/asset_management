use diesel::table;

table! {
    currency (currency_id) {
        currency_id -> Integer,
        symbol -> VarChar,
        name -> VarChar,
    }
}

table! {
    stamp (stamp_id) {
        stamp_id -> Integer,
        #[sql_name = "stamp"]
        timestamp -> Timestamp,
    }
}

table! {
    balance (balance_id) {
        balance_id -> Integer,
        currency_id -> Integer,
        stamp_id -> Integer,
        available -> Float,
        pending -> Float,
    }
}

joinable!(balance -> currency(currency_id));
allow_tables_to_appear_in_same_query!(currency, balance);

table! {
    market (market_id) {
        market_id -> Integer,
        base_id -> Integer,
        quote_id -> Integer,
    }
}

table! {
    price (price_id) {
        price_id -> Integer,
        market_id -> Integer,
        stamp_id -> Integer,
        #[sql_name = "price"]
        amount -> Float,
    }
}

joinable!(price -> market(market_id));
allow_tables_to_appear_in_same_query!(market, price);
joinable!(price -> stamp(stamp_id));
allow_tables_to_appear_in_same_query!(stamp, price);
allow_tables_to_appear_in_same_query!(market, stamp);

table! {
    use diesel::sql_types::*;
    use crate::custom_sql_type::*;

    orderbook (orderbook_id) {
        orderbook_id -> Integer,
        market_id -> Integer,
        stamp_id -> Integer,
        side -> OrderSideMapping,
        price -> Float,
        volume -> Float,
    }
}

joinable!(orderbook -> market(market_id));
allow_tables_to_appear_in_same_query!(market, orderbook);
joinable!(orderbook -> stamp(stamp_id));
allow_tables_to_appear_in_same_query!(stamp, orderbook);

table! {
    use diesel::sql_types::*;
    use crate::custom_sql_type::*;

    myorder (myorder_id) {
        myorder_id -> Integer,
        transaction_id -> VarChar,
        market_id -> Integer,
        created_stamp_id -> Integer,
        modified_stamp_id -> Integer,
        price -> Float,
        base_quantity -> Float,
        quote_quantity -> Float,
        order_type -> OrderTypeMapping,
        side -> OrderSideMapping,
        state -> OrderStateMapping,
    }
}

joinable!(myorder -> market(market_id));
allow_tables_to_appear_in_same_query!(market, myorder);

table! {
    next_id (currency) {
        currency -> Integer,
        stamp -> Integer,
        balance -> Integer,
        market -> Integer,
        price -> Integer,
        orderbook -> Integer,
        myorder -> Integer,
    }
}

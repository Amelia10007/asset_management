use diesel::table;

table! {
    currency (currency_id) {
        currency_id -> Integer,
        symbol -> VarChar,
        name -> VarChar,
    }
}

table! {
    balance (balance_id) {
        balance_id -> Integer,
        currency_id -> Integer,
        stamp -> Timestamp,
        #[sql_name = "balance"]
        amount -> Float,
    }
}

joinable!(balance -> currency(currency_id));

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
        stamp -> Timestamp,
        #[sql_name = "price"]
        amount -> Float,
    }
}

joinable!(price -> market(market_id));

table! {
    orderbook (orderbook_id) {
        orderbook_id -> Integer,
        market_id -> Integer,
        stamp -> Timestamp,
        is_buy -> Bool,
        price -> Float,
        volume -> Float,
    }
}

joinable!(orderbook -> market(market_id));

table! {
    myorder (myorder_id) {
        myorder_id -> Integer,
        transaction_id -> VarChar,
        market_id -> Integer,
        created -> Timestamp,
        modified -> Timestamp,
        price -> Float,
        base_quantity -> Float,
        quote_quantity -> Float,
        state -> VarChar,
    }
}

joinable!(myorder -> market(market_id));

table! {
    next_id (dummy_id) {
        dummy_id -> Integer,
        currency -> Integer,
        balance -> Integer,
        market -> Integer,
        price -> Integer,
        orderbook -> Integer,
        myorder -> Integer,
    }
}

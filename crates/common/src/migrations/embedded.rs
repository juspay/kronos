//! Migration template files embedded at compile time. The order in
//! `MIGRATIONS` matches the order they must be applied.

pub struct Migration {
    pub name: &'static str,
    pub template: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        name: "20260317000000_initial",
        template: include_str!("../../../../migrations/20260317000000_initial.sql"),
    },
    Migration {
        name: "20260318000000_multi_tenancy",
        template: include_str!("../../../../migrations/20260318000000_multi_tenancy.sql"),
    },
    Migration {
        name: "20260322000000_txn_based_pickup",
        template: include_str!("../../../../migrations/20260322000000_txn_based_pickup.sql"),
    },
    Migration {
        name: "20260322000001_pg_cron",
        template: include_str!("../../../../migrations/20260322000001_pg_cron.sql"),
    },
];

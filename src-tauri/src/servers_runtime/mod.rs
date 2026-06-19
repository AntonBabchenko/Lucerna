//! «Свой сервер» — сборка и (План 2) запуск изолированного MC-сервера.
//! Сущность отдельная от инстанса; артефакты живут в `<app_data>/servers/<id>/`.

pub mod backup;
pub mod create;
pub mod eula;
pub mod import;
pub mod jar;
pub mod pid;
pub mod preflight;
pub mod properties;
pub mod runtime;
pub mod schema;
pub mod store;
pub mod to_instance;
pub mod transfer;

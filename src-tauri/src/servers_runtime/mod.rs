//! «Свой сервер» — сборка и (План 2) запуск изолированного MC-сервера.
//! Сущность отдельная от инстанса; артефакты живут в `<app_data>/servers/<id>/`.

pub mod backup;
pub mod create;
pub mod datapacks;
pub mod eula;
pub mod exit_state;
pub mod firewall;
pub mod import;
pub mod jar;
pub mod mod_classify;
pub mod paper;
pub mod pid;
pub mod preflight;
pub mod properties;
pub mod quarantine;
pub mod runtime;
pub mod schema;
pub mod serverlog;
pub mod store;
pub mod to_instance;
pub mod transfer;
pub mod upload_control;
pub mod upload_manifest;
pub mod whitelist;

//! «Свой сервер» — сборка и (План 2) запуск изолированного MC-сервера.
//! Сущность отдельная от инстанса; артефакты живут в `<app_data>/servers/<id>/`.

pub mod create;
pub mod eula;
pub mod jar;
pub mod properties;
pub mod schema;
pub mod store;

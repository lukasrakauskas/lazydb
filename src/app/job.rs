use std::collections::HashMap;
use std::sync::mpsc::Receiver;

use crate::db::{Database, ExecCtx, ExecutionResult, TriggerInfo};

pub enum Job {
    Ping(Box<dyn Database>, String),
    Query(Box<dyn Database>, String, ExecCtx),
    Schema(Box<dyn Database>),
    Views(Box<dyn Database>),
    Procedures(Box<dyn Database>),
    Triggers(Box<dyn Database>),
    PrimaryKeys(Box<dyn Database>, String),
    UpdateCell(Box<dyn Database>, String, ExecCtx),
}

pub enum JobResult {
    Ping(Result<String, String>),
    Query(Result<ExecutionResult, String>),
    Schema(Result<HashMap<String, Vec<String>>, String>),
    Views(Result<Vec<String>, String>),
    Procedures(Result<Vec<String>, String>),
    Triggers(Result<Vec<TriggerInfo>, String>),
    PrimaryKeys(Result<Vec<String>, String>),
    UpdateCell(Result<ExecutionResult, String>),
}

pub fn spawn_job(job: Job) -> Receiver<JobResult> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let res = match job {
            Job::Ping(db, name) => match db.ping() {
                Ok(()) => JobResult::Ping(Ok(name)),
                Err(e) => {
                    let s = e.to_string();
                    crate::log::error("job_ping_err", &[("err", &s)]);
                    JobResult::Ping(Err(s))
                }
            },
            Job::Query(db, sql, ctx) => {
                let start = std::time::Instant::now();
                match db.execute_script(&sql, &ctx) {
                    Ok(mut r) => {
                        r.elapsed_ms = start.elapsed().as_millis();
                        JobResult::Query(Ok(r))
                    }
                    Err(e) => {
                        let s = e.to_string();
                        crate::log::error("job_query_err", &[("err", &s)]);
                        JobResult::Query(Err(s))
                    }
                }
            }
            Job::Schema(db) => match db.schema() {
                Ok(s) => JobResult::Schema(Ok(s)),
                Err(e) => JobResult::Schema(Err(e.to_string())),
            },
            Job::Views(db) => match db.views() {
                Ok(v) => JobResult::Views(Ok(v)),
                Err(e) => JobResult::Views(Err(e.to_string())),
            },
            Job::Procedures(db) => match db.procedures() {
                Ok(p) => JobResult::Procedures(Ok(p)),
                Err(e) => JobResult::Procedures(Err(e.to_string())),
            },
            Job::Triggers(db) => match db.triggers() {
                Ok(t) => JobResult::Triggers(Ok(t)),
                Err(e) => JobResult::Triggers(Err(e.to_string())),
            },
            Job::PrimaryKeys(db, table) => match db.primary_keys(&table) {
                Ok(pks) => JobResult::PrimaryKeys(Ok(pks)),
                Err(e) => JobResult::PrimaryKeys(Err(e.to_string())),
            },
            Job::UpdateCell(db, sql, ctx) => {
                let start = std::time::Instant::now();
                match db.execute_script(&sql, &ctx) {
                    Ok(mut r) => {
                        r.elapsed_ms = start.elapsed().as_millis();
                        JobResult::UpdateCell(Ok(r))
                    }
                    Err(e) => JobResult::UpdateCell(Err(e.to_string())),
                }
            }
        };
        let _ = tx.send(res);
    });
    rx
}

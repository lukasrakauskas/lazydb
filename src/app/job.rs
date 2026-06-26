use std::collections::HashMap;
use std::sync::mpsc::Receiver;

use crate::db::{Database, ExecutionResult};

pub enum Job {
    Ping(Box<dyn Database>, String),
    Query(Box<dyn Database>, String, bool),
    Schema(Box<dyn Database>),
    PrimaryKeys(Box<dyn Database>, String),
    UpdateCell(Box<dyn Database>, String),
}

pub enum JobResult {
    Ping(Result<String, String>),
    Query(Result<ExecutionResult, String>),
    Schema(Result<HashMap<String, Vec<String>>, String>),
    PrimaryKeys(Result<Vec<String>, String>),
    UpdateCell(Result<ExecutionResult, String>),
}

pub fn spawn_job(job: Job) -> Receiver<JobResult> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let res = match job {
            Job::Ping(db, name) => match db.ping() {
                Ok(()) => JobResult::Ping(Ok(name)),
                Err(e) => JobResult::Ping(Err(e.to_string())),
            },
            Job::Query(db, sql, readable_binary) => {
                let start = std::time::Instant::now();
                match db.execute_script(&sql, readable_binary) {
                    Ok(mut r) => {
                        r.elapsed_ms = start.elapsed().as_millis();
                        JobResult::Query(Ok(r))
                    }
                    Err(e) => JobResult::Query(Err(e.to_string())),
                }
            }
            Job::Schema(db) => match db.schema() {
                Ok(s) => JobResult::Schema(Ok(s)),
                Err(e) => JobResult::Schema(Err(e.to_string())),
            },
            Job::PrimaryKeys(db, table) => match db.primary_keys(&table) {
                Ok(pks) => JobResult::PrimaryKeys(Ok(pks)),
                Err(e) => JobResult::PrimaryKeys(Err(e.to_string())),
            },
            Job::UpdateCell(db, sql) => {
                let start = std::time::Instant::now();
                match db.execute_script(&sql, false) {
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

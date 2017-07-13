#![feature(plugin)]
#![plugin(rocket_codegen)]
extern crate rocket;
#[macro_use]
extern crate rocket_contrib;
extern crate babystats;

use std::error::Error;
use std::io;
use std::process;
use std::sync::Mutex;
use babystats::BabyManagerData;
use rocket::State;
use rocket_contrib::Json;

#[get("/")]
fn index(events: State<Mutex<Vec<babystats::Event>>>) -> Json<Vec<babystats::Event>> {
    Json(*events.lock().unwrap())
}

fn run() -> Result<(), Box<Error>> {
    println!("Hello, world!");
    let mut rdr = BabyManagerData::from_reader(io::stdin());
    let events: Vec<_> = rdr.into_iter().map(|r| r.unwrap()).collect();
    rocket::ignite()
        .manage(Mutex::new(events))
        .mount("/", routes![index])
        .launch();
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        println!("{}", err);
        process::exit(1);
    }
}

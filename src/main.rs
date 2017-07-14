#![feature(plugin)]
#![plugin(rocket_codegen)]
extern crate rocket;
extern crate rocket_contrib;
extern crate chrono;
extern crate babystats;
extern crate serde;
#[macro_use]
extern crate serde_derive;

use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::ops::Deref;
use std::process;
use std::sync::Mutex;
use babystats::BabyManagerData;
use rocket::State;
use rocket_contrib::Json;

#[get("/")]
fn index(events: State<Mutex<Vec<babystats::Event>>>) -> Json<Vec<babystats::Event>> {
    let temp: Vec<_> = events.lock().unwrap().clone();
    Json(temp)
}

#[get("/graph/<name>")]
fn graph(name: GraphType, shared_events: State<Mutex<Vec<babystats::Event>>>) -> Json<plotly::Data<f64>> {
    let events = shared_events.lock().unwrap();
    Json(name.data(events.deref()))
}

enum GraphType {
    Milk
}

impl<'a> rocket::request::FromParam<'a> for GraphType {
    type Error = String;
    fn from_param(param: &'a rocket::http::RawStr) -> Result<GraphType, String> {
        let g = match param.as_str() {
            "milk" => GraphType::Milk,
            _ => return Err(format!("unknown graph type: {}", param)),
        };
        Ok(g)
    }
}

impl GraphType {
    fn data(&self, events: &Vec<babystats::Event>) -> plotly::Data<f64> {
        match *self {
            GraphType::Milk => self.milk_data(events),
        }
    }

    fn milk_data(&self, events: &Vec<babystats::Event>) -> plotly::Data<f64> {
        let mut m: BTreeMap<_, _> = BTreeMap::new();
        for event in events.iter().filter_map(|e| match *e {
            babystats::Event::Feeding(babystats::FeedingEvent::Bottle(ref be)) => Some(be),
            _ => None,
        }) {
            let amount = m.entry(event.time.date()).or_insert(0.0);
            *amount += event.ounces;
        }
        vec!(plotly::Trace{
                x: m.keys().map(|d| d.and_hms(0,0,0)).collect(),
                y: m.values().map(|x| x.clone() as f64).collect(),
                mode: "lines".to_string(),
            })
    }
}

mod plotly {
    use chrono;
    use chrono::Local;
    
    #[derive(Debug,Serialize)]
    pub struct Trace<T> {
        pub x: Vec<chrono::DateTime<Local>>,
        pub y: Vec<T>,
        pub mode: String,
    }

    pub type Data<T> = Vec<Trace<T>>;

    #[derive(Debug,Serialize)]
    pub struct Layount {
        pub title: String,
    }

}

fn run() -> Result<(), Box<Error>> {
    println!("Hello, world!");
    let mut rdr = BabyManagerData::from_reader(io::stdin());
    let events: Vec<_> = rdr.into_iter().map(|r| r.unwrap()).collect();
    rocket::ignite()
        .manage(Mutex::new(events))
        .mount("/", routes![index, graph])
        .launch();
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        println!("{}", err);
        process::exit(1);
    }
}

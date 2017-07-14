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

#[derive(Debug,Serialize)]
struct GraphContext {
    foo: i32,
}

#[get("/graph/<name>")]
fn graph(name: GraphType) -> rocket_contrib::Template {
    rocket_contrib::Template::render("graph", GraphContext{foo: 1})
}

#[get("/table/<name>")]
fn table(name: GraphType) -> rocket_contrib::Template {
    rocket_contrib::Template::render("table", GraphContext{foo: 1})
}

#[get("/graph/<name>/data")]
fn data(name: GraphType, shared_events: State<Mutex<Vec<babystats::Event>>>) -> Json<plotly::Data<f64>> {
    let events = shared_events.lock().unwrap();
    Json(name.data(events.deref()))
}

#[get("/graph/<name>/layout")]
fn layout(name: GraphType) -> Json<plotly::Layout> {
    Json(name.layout())
}

enum GraphType {
    Bottle,
    MaxSleep,
}

impl<'a> rocket::request::FromParam<'a> for GraphType {
    type Error = String;
    fn from_param(param: &'a rocket::http::RawStr) -> Result<GraphType, String> {
        let g = match param.as_str() {
            "bottle" => GraphType::Bottle,
            "maxsleep" => GraphType::MaxSleep,
            _ => return Err(format!("unknown graph type: {}", param)),
        };
        Ok(g)
    }
}

impl GraphType {
    fn data(&self, events: &Vec<babystats::Event>) -> plotly::Data<f64> {
        match *self {
            GraphType::Bottle => self.bottle_data(events),
            GraphType::MaxSleep => self.max_sleep_data(events),
        }
    }

    fn layout(&self) -> plotly::Layout {
        match *self {
            GraphType::Bottle => self.bottle_layout(),
            GraphType::MaxSleep => self.max_sleep_layout(),
        }
    }

    fn bottle_data(&self, events: &Vec<babystats::Event>) -> plotly::Data<f64> {
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

    fn bottle_layout(&self) -> plotly::Layout {
        plotly::Layout{
            title: "Bottles per day".to_string(),
            xaxis: None,
            yaxis: Some(plotly::Axis{title: "Ounces".to_string()})
        }
    }

    fn max_sleep_data(&self, events: &Vec<babystats::Event>) -> plotly::Data<f64> {
        let mut m: BTreeMap<_, _> = BTreeMap::new();
        for (date, duration) in events.iter().filter_map(|e| match *e {
            babystats::Event::Sleep(babystats::SleepEvent{end: Some(ref end), ref duration, ..}) => Some((end, duration)),
            _ => None,
        }) {
            let hours = duration.num_milliseconds() as f64 / 3600000.0;
            let x = m.entry(date.date()).or_insert(0.0);
            if *x < hours {
                *x = hours;
            }
        }
        let v: Vec<_> = m.into_iter().collect();
        let rolling_mean: Vec<_> = v.windows(5).map(|kv| {
            let &(k,_) = kv.iter().last().unwrap();
            let (count, sum) = kv.iter().fold((0, 0.0), |(count, sum), &(_, v)| {
                (count+1, sum + v)
            });
            (k, sum / count as f64)
        }).collect();
        vec!(plotly::Trace{
                x: rolling_mean.iter().map(|&(k,_)| k.and_hms(0,0,0)).collect(),
                y: rolling_mean.iter().map(|&(_,v)| v).collect(),
                mode: "lines".to_string(),
            })
    }

    fn max_sleep_layout(&self) -> plotly::Layout {
        plotly::Layout{
            title: "Max sleep duration per night".to_string(),
            xaxis: None,
            yaxis: Some(plotly::Axis{title: "Hours".to_string()})
        }
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
    pub struct Layout {
        pub title: String,
        #[serde(skip_serializing_if="Option::is_none")]
        pub xaxis: Option<Axis>,
        #[serde(skip_serializing_if="Option::is_none")]
        pub yaxis: Option<Axis>,
    }

    #[derive(Debug,Serialize)]
    pub struct Axis {
        pub title: String,
    }
}

fn run() -> Result<(), Box<Error>> {
    println!("Hello, world!");
    let mut rdr = BabyManagerData::from_reader(io::stdin());
    let events: Vec<_> = rdr.into_iter().map(|r| r.unwrap()).collect();
    rocket::ignite()
        .manage(Mutex::new(events))
        .attach(rocket_contrib::Template::fairing())
        .mount("/", routes![index, graph, table, data, layout])
        .launch();
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        println!("{}", err);
        process::exit(1);
    }
}

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
        for event in events {
            match *event {
                babystats::Event::Feeding(babystats::FeedingEvent::Bottle(ref be)) => {
                    let amount = m.entry(be.time.date()).or_insert(FeedingTotals::new());
                    match be.milk {
                        babystats::Milk::BreastMilk => amount.breast_milk += be.ounces as f64,
                        babystats::Milk::Formula => amount.formula += be.ounces as f64,
                        babystats::Milk::Unknown => amount.unknown += be.ounces as f64,
                    };
                },
                babystats::Event::Feeding(babystats::FeedingEvent::LeftBreast(ref le)) => {
                    let amount = m.entry(le.start.date()).or_insert(FeedingTotals::new());
                    amount.breast_feeding = amount.breast_feeding + le.duration;
                },
                babystats::Event::Feeding(babystats::FeedingEvent::RightBreast(ref re)) => {
                    let amount = m.entry(re.start.date()).or_insert(FeedingTotals::new());
                    amount.breast_feeding = amount.breast_feeding + re.duration;
                },
                _ => {},
            }
        }
        vec!(plotly::Trace{
                x: m.keys().map(|d| d.and_hms(0,0,0)).collect(),
                y: m.values().map(|x| x.breast_milk).collect(),
                mode: None,
                name: Some("Breast Milk".to_string()),
                typ: Some("bar".to_string()),
            },
            plotly::Trace{
                x: m.keys().map(|d| d.and_hms(0,0,0)).collect(),
                y: m.values().map(|x| x.formula).collect(),
                mode: None,
                name: Some("Formula".to_string()),
                typ: Some("bar".to_string()),
            },
            plotly::Trace{
                x: m.keys().map(|d| d.and_hms(0,0,0)).collect(),
                y: m.values().map(|x| x.unknown).collect(),
                mode: None,
                name: Some("Unknown".to_string()),
                typ: Some("bar".to_string()),
            })
    }

    fn bottle_layout(&self) -> plotly::Layout {
        plotly::Layout{
            title: "Bottles per day".to_string(),
            xaxis: None,
            yaxis: Some(plotly::Axis{title: "Ounces".to_string()}),
            barmode: Some("stack".to_string()),
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
        let rolling_min_mean_max: Vec<_> = v.windows(5).map(|kv| {
            let &(k,_) = kv.iter().last().unwrap();
            let min_mean_max = kv.iter().fold(MinMeanMax::new(), |mut mmm, &(_, v)| {
                mmm.record(v);
                mmm
            });
            (k, min_mean_max)
        }).collect();
        vec!(plotly::Trace{
                x: rolling_min_mean_max.iter().map(|&(k,_)| k.and_hms(0,0,0)).collect(),
                y: rolling_min_mean_max.iter().map(|&(_,ref v)| v.min().unwrap()).collect(),
                mode: Some("lines".to_string()),
                typ: None,
                name: Some("min".to_string()),
            },
            plotly::Trace{
                x: rolling_min_mean_max.iter().map(|&(k,_)| k.and_hms(0,0,0)).collect(),
                y: rolling_min_mean_max.iter().map(|&(_,ref v)| v.mean().unwrap()).collect(),
                mode: Some("lines".to_string()),
                typ: None,
                name: Some("mean".to_string()),
            },
            plotly::Trace{
                x: rolling_min_mean_max.iter().map(|&(k,_)| k.and_hms(0,0,0)).collect(),
                y: rolling_min_mean_max.iter().map(|&(_,ref v)| v.max().unwrap()).collect(),
                mode: Some("lines".to_string()),
                typ: None,
                name: Some("max".to_string()),
            })
    }

    fn max_sleep_layout(&self) -> plotly::Layout {
        plotly::Layout{
            title: "Max sleep duration per night".to_string(),
            xaxis: None,
            yaxis: Some(plotly::Axis{title: "Hours".to_string()}),
            barmode: None,
        }
    }
}

struct MinMeanMax {
    ct: i64,
    sm: Option<f64>,
    mn: Option<f64>,
    mx: Option<f64>,
}

impl MinMeanMax {
    fn new() -> MinMeanMax {
        MinMeanMax{
            ct: 0,
            sm: None,
            mn: None,
            mx: None,
        }
    }

    fn record(&mut self, x: f64) {
        self.ct += 1;
        self.sm = Some(self.sm.unwrap_or(0.0) + x);
        self.mn = match self.mn {
            Some(m) if x < m => Some(x),
            None => Some(x),
            _ => self.mn,
        };
        self.mx = match self.mx {
            Some(m) if x > m => Some(x),
            None => Some(x),
            _ => self.mx,
        };
    }

    fn mean(&self) -> Option<f64> {
        if let Some(sum) = self.sm {
            Some(sum / self.ct as f64)
        } else {
            None
        }
    }

    fn max(&self) -> Option<f64> {
        self.mx
    }

    fn min(&self) -> Option<f64> {
        self.mn
    }
}

struct FeedingTotals {
    unknown: f64,
    breast_milk: f64,
    formula: f64,
    breast_feeding: chrono::Duration,
}

impl FeedingTotals {
    fn new() -> FeedingTotals {
        FeedingTotals{
            unknown: 0.0,
            breast_milk: 0.0,
            formula: 0.0,
            breast_feeding: chrono::Duration::seconds(0),
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
        #[serde(skip_serializing_if="Option::is_none")]
        pub mode: Option<String>,
        #[serde(skip_serializing_if="Option::is_none")]
        pub name: Option<String>,
        #[serde(skip_serializing_if="Option::is_none",rename="type")]
        pub typ: Option<String>,
    }

    pub type Data<T> = Vec<Trace<T>>;

    #[derive(Debug,Serialize)]
    pub struct Layout {
        pub title: String,
        #[serde(skip_serializing_if="Option::is_none")]
        pub xaxis: Option<Axis>,
        #[serde(skip_serializing_if="Option::is_none")]
        pub yaxis: Option<Axis>,
        #[serde(skip_serializing_if="Option::is_none")]
        pub barmode: Option<String>,
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

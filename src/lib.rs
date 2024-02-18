use std::{
    convert::Infallible,
    error::Error,
    iter,
    net::ToSocketAddrs,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use futures::{stream::FuturesUnordered, StreamExt};
use hyper::{client::HttpConnector, Body, Method, Request, Response, Uri};

pub use result::BenchmarkResult;
pub use uri::UriExt;

mod result;
mod uri;
pub mod http {
    pub use hyper::{header, Body, Method, Request, Response, StatusCode, Uri};
}

type MakeRequest = Arc<dyn Fn(&Uri) -> Request<Body> + Send + Sync + 'static>;
type Expectation = Arc<dyn Fn(Response<Body>) -> bool + Send + Sync + 'static>;

pub fn swarm<T>(uri: T) -> SwarmBuilder
where
    Uri: TryFrom<T>,
    <Uri as TryFrom<T>>::Error: Into<Box<dyn Error + Send + Sync>>,
{
    Swarm::builder().uri(uri)
}

pub struct Swarm {
    uri: Uri,
    duration: Duration,
    threads: usize,
    concurrency: usize,
    make_request: MakeRequest,
    expectation_matcher: Expectation,
}

impl Swarm {
    pub fn builder() -> SwarmBuilder {
        SwarmBuilder::default()
    }

    pub fn zerg(self) -> BenchmarkResult {
        let running = Arc::new(AtomicBool::new(false));

        let host = self.uri.authority().map(|auth| auth.to_string()).unwrap();
        let addr = host.to_socket_addrs().unwrap().next().unwrap();

        let dns = tower::service_fn(move |_| async move { Ok::<_, Infallible>(iter::once(addr)) });

        let uri = Arc::new(self.uri);

        let results = (0..self.threads)
            .map(|_| {
                let running = running.clone();
                let uri = uri.clone();
                let make_request = self.make_request.clone();
                let expectation_matcher = self.expectation_matcher.clone();

                std::thread::spawn(move || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();

                    let results = (0..self.concurrency / self.threads).map(|_| {
                        let uri = uri.clone();
                        let running = running.clone();
                        let make_request = make_request.clone();
                        let expectation_matcher = expectation_matcher.clone();

                        async move {
                            let mut http_connector = HttpConnector::new_with_resolver(dns);
                            http_connector.set_nodelay(true);

                            let http: hyper::Client<_, hyper::Body> =
                                hyper::Client::builder().build(http_connector);

                            let mut result = BenchmarkResult::default();

                            while running.load(Ordering::Relaxed) {
                                let start = Instant::now();
                                let req = (make_request)(&uri);
                                match http.request(req).await {
                                    Ok(res) => {
                                        if (expectation_matcher)(res) {
                                            result.success += 1;
                                        } else {
                                            result.http_error += 1;
                                        }
                                    }
                                    Err(_) => result.tcp_error += 1,
                                }
                                let elapsed = start.elapsed();
                                result.elapsed = elapsed;
                                result.timings.push(elapsed);
                                result.min_time = result.min_time.min(elapsed);
                                result.max_time = result.max_time.max(elapsed);
                            }

                            result
                        }
                    });

                    let results = FuturesUnordered::from_iter(results).collect::<Vec<_>>();
                    let results = runtime.block_on(results);
                    results.into_iter().sum()
                })
            })
            .collect::<Vec<thread::JoinHandle<_>>>();

        running.store(true, Ordering::Relaxed);
        let start = Instant::now();
        thread::sleep(self.duration);
        running.store(false, Ordering::Relaxed);
        let elapsed = start.elapsed();

        let mut results = results
            .into_iter()
            .filter_map(|t| match t.join() {
                Ok(results) => Some(results),
                _ => None,
            })
            .sum::<BenchmarkResult>();

        results.elapsed = elapsed;
        results
    }
}

pub struct SwarmBuilder {
    uri: Result<Uri, Box<dyn Error + Send + Sync>>,
    duration: Duration,
    threads: usize,
    concurrency: usize,
    make_request: MakeRequest,
    expectation_matcher: Expectation,
}

impl Default for SwarmBuilder {
    fn default() -> Self {
        Self {
            uri: Err("missing uri".into()),
            duration: Duration::from_secs(1),
            threads: 1,
            concurrency: 100,
            make_request: Arc::new(|uri| {
                Request::builder()
                    .uri(uri)
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap()
            }),
            expectation_matcher: Arc::new(|res| res.status().is_success()),
        }
    }
}

impl SwarmBuilder {
    pub fn uri<T>(self, uri: T) -> Self
    where
        Uri: TryFrom<T>,
        <Uri as TryFrom<T>>::Error: Into<Box<dyn Error + Send + Sync>>,
    {
        Self {
            uri: TryFrom::try_from(uri).map_err(Into::into),
            ..self
        }
    }

    pub fn duration(self, duration: Duration) -> Self {
        Self { duration, ..self }
    }

    pub fn threads(self, threads: usize) -> Self {
        Self { threads, ..self }
    }

    pub fn concurrency(self, concurrency: usize) -> Self {
        Self {
            concurrency,
            ..self
        }
    }

    pub fn request(self, f: impl Fn(&Uri) -> Request<Body> + Send + Sync + 'static) -> Self {
        Self {
            make_request: Arc::new(f),
            ..self
        }
    }

    pub fn expecting(self, f: impl Fn(Response<Body>) -> bool + Send + Sync + 'static) -> Self {
        Self {
            expectation_matcher: Arc::new(f),
            ..self
        }
    }

    pub fn build(self) -> Result<Swarm, Box<dyn Error + Send + Sync>> {
        Ok(Swarm {
            uri: self.uri?,
            duration: self.duration,
            threads: self.threads,
            concurrency: self.concurrency,
            make_request: self.make_request,
            expectation_matcher: self.expectation_matcher,
        })
    }

    pub fn zerg(self) -> Result<BenchmarkResult, Box<dyn Error + Send + Sync>> {
        self.build().map(|swarm| swarm.zerg())
    }
}

#[macro_export]
macro_rules! json {
    ($some_json:tt) => {
        serde_json::to_vec(&serde_json::json!($some_json)).unwrap()
    };
}

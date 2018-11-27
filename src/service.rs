use super::config::{Config, Networks, Network};
use exe_common::{sync, parse_genesis_data, genesis_data};
use super::handlers;
use iron;
use router::Router;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use exe_common::config::net;

pub fn start(cfg: Config) {
    let _refreshers = if cfg.sync { Some(start_networks_refreshers(cfg.clone())) } else { None };
    let _server = start_http_server(&cfg, Arc::new(cfg.get_networks().unwrap()));

    // XXX: consider installing a signal handler to initiate a graceful shutdown here
    // XXX: after initiating shutdown, do `refresher.join()` and something similar for `server`.
}

fn start_http_server(cfg: &Config, networks: Arc<Networks>) -> iron::Listening {
    let mut router = Router::new();
    handlers::block::Handler::new(networks.clone()).route(&mut router);
    handlers::pack::Handler::new(networks.clone()).route(&mut router);
    handlers::epoch::Handler::new(networks.clone()).route(&mut router);
    handlers::tip::Handler::new(networks.clone()).route(&mut router);
    handlers::chain_state::Handler::new(networks.clone()).route(&mut router);
    handlers::chain_state_delta::Handler::new(networks.clone()).route(&mut router);
    info!("listening to port {}", cfg.port);
    iron::Iron::new(router)
        .http(format!("0.0.0.0:{}", cfg.port))
        .expect("start http server")
}

// TODO: make this a struct which receives a shutdown message on a channel and then wraps itself up
fn start_networks_refreshers(cfg: Config) -> Vec<thread::JoinHandle<()>> {
    let mut threads = vec![];
    match cfg.get_networks() {
        Err(err) => panic!("Unable to get networks: {:?}", err),
        Ok(networks) => {
            for (label, net) in networks.into_iter() {
                threads.push(thread::spawn(move || {
                    loop {
                        refresh_network(&label, &net);
                        // In case of an error, wait a while before retrying.
                        thread::sleep(Duration::from_secs(10));
                    }
                }));
            }
        }
    }
    threads
}

// XXX: how do we want to report partial failures?
fn refresh_network(label: &str, net: &Network) {
    info!("Refreshing network {:?}", label);

    let mut storage = net.storage.write().unwrap();

    let netcfg_file = storage.config.get_config_file();
    let net_cfg = net::Config::from_file(&netcfg_file).expect("no network config present");

    let genesis_data = {
        let genesis_data = genesis_data::get_genesis_data(&net_cfg.genesis_prev)
            .expect("genesis data not found");
        parse_genesis_data::parse_genesis_data(genesis_data.as_bytes())
    };

    // FIXME: this locks net.storage for a long time. Maybe we should clone it?
    sync::net_sync(&mut sync::get_peer(&label, &net_cfg, true), &net_cfg,
                   &genesis_data, &mut storage, false)
        .unwrap_or_else(|err| { warn!("Sync failed: {:?}", err) });
}

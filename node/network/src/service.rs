use futures::{self, stream, Future, Stream};
use tokio::timer::Interval;
use std::io;
use std::sync::Arc;
use std::time::Duration;
use parking_lot::Mutex;
use substrate_network_libp2p::{
    start_service, Service as NetworkService, ServiceEvent,
    NetworkConfiguration, ProtocolId, RegisteredProtocol,
};
use protocol::{self, Protocol, ProtocolConfig, Transaction};
use error::Error; 

const TICK_TIMEOUT: Duration = Duration::from_millis(1000);

pub struct Service {
    network: Arc<Mutex<NetworkService>>,
    protocol: Arc<Protocol>,
}

impl Service {
    pub fn new<T: Transaction>(
        config: ProtocolConfig,
        net_config: NetworkConfiguration,
        protocol_id: ProtocolId
    ) -> Result<Service, Error> {
        let version = [protocol::CURRENT_VERSION as u8];
        let registered = RegisteredProtocol::new(protocol_id, &version);
        let protocol = Arc::new(Protocol::new(config));
        let service = match start_service(net_config, Some(registered)) {
            Ok(s) => s,
            Err(e) => return Err(e.into())
        };
        Ok(Service {
            network: Arc::new(Mutex::new(service)),
            protocol,
        })
    }

    pub fn service_task<T: Transaction>(&self) -> impl Future<Item = (), Error = io::Error> {
        let network_service1 = self.network.clone();
        let network = stream::poll_fn(move || network_service1.lock().poll()).for_each({
            let protocol = self.protocol.clone();
            let network_service = self.network.clone();
            move |event| {
            debug!(target: "sub-libp2p", "event: {:?}", event);
            match event {
                ServiceEvent::CustomMessage { node_index, data, .. } => {
                    protocol.on_message::<T>(node_index, &data);
                },
                ServiceEvent::OpenedCustomProtocol { node_index, .. } => {
                    protocol.on_peer_connected::<T>(&network_service, node_index);
                },
                ServiceEvent::ClosedCustomProtocol { node_index, .. } => {
                    protocol.on_peer_disconnected(node_index);
                },
                _ => {
                    debug!("TODO");
                    ()
                }
            };
            Ok(())
        }});

        // Interval for performing maintenance on the protocol handler.
    	let timer = Interval::new_interval(TICK_TIMEOUT)
    		.for_each({
    			let protocol = self.protocol.clone();
    			let network_service = self.network.clone();
    			move |_| {
    				protocol.maintain_peers(&network_service);
    				Ok(())
    			}
    		})
    		.then(|res| {
    			match res {
    				Ok(()) => (),
    				Err(err) => error!("Error in the propagation timer: {:?}", err),
    			};
    			Ok(())
    		});


        let futures: Vec<Box<Future<Item = (), Error = io::Error> + Send>> = vec![
            Box::new(network),
            Box::new(timer),
        ];

        futures::select_all(futures)
    		.and_then(move |_| {
    			info!("Networking ended");
    			Ok(())
    		})
    		.map_err(|(r, _, _)| r)
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use test_utils::*;
    use rand::Rng;
    use std::time;
    use primitives::types;
    use std::thread;
    use tokio::runtime::Runtime;

    fn create_services(num_services: u32) -> Vec<Service> {
        let base_address = "/ip4/127.0.0.1/tcp/".to_string();
        let base_port = rand::thread_rng().gen_range(30000, 60000);
        let mut addresses = Vec::new();
        for i in 0..num_services {
            let port = base_port + i;
            addresses.push(base_address.clone() + &port.to_string());
        }
        // spin up a root service that does not have bootnodes and 
        // have other services have this service as their boot node
        // may want to abstract this out to enable different configurations
        let secret = create_secret();
        let root_config = test_config_with_secret(&addresses[0], vec![], secret);
        let root_service = Service::new::<types::SignedTransaction>(
            ProtocolConfig::default(), root_config, ProtocolId::default()
        ).unwrap();
        let boot_node = addresses[0].clone() + "/p2p/" + &raw_key_to_peer_id_str(secret);
        let mut services = vec![root_service];
        for i in 1..num_services {
            let config = test_config(&addresses[i as usize], vec![boot_node.clone()]);
            let service = Service::new::<types::SignedTransaction>(
                ProtocolConfig::default(), config, ProtocolId::default()
            ).unwrap();
            services.push(service);
        }
        services
    }

    #[test]
    fn test_send_message() {
        let services = create_services(2);
        thread::sleep(time::Duration::from_secs(1));
        let mut runtime = tokio::runtime::Runtime::new().unwrap();
        let _ = services.iter().map(|s| {
            runtime.spawn(s.service_task::<types::SignedTransaction>().map_err(|_| ()));
        }).collect::<Vec<_>>();
        thread::sleep(time::Duration::from_millis(100));
        for service in services {
            for peer in service.protocol.sample_peers(1) {
                let message = fake_tx_message();
                service.protocol.send_message(&service.network, peer, message);
            }
        }
        thread::sleep(time::Duration::from_millis(100));
    }
}
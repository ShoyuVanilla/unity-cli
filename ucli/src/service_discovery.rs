use std::{
    net::SocketAddrV4,
    path::PathBuf,
    str::FromStr,
    time::{Duration, Instant},
};

use common::{
    MDNS_SERVICE_NAME, PROJECT_NAME_PROP_KEY, PROJECT_PATH_PROP_KEY, UNITY_VERSION_PROP_KEY,
};
use mdns_sd::{IPMulticastTTLOption, ServiceDaemon, ServiceEvent, ServiceInfo};

use crate::cli_args::DiscoveryArgs;

pub struct UnityService {
    address: SocketAddrV4,
    hostname: String,
    path: PathBuf,
    project: String,
    unity_version: String,
    session_name: String,
}

pub fn discover_service(args: DiscoveryArgs) -> Vec<UnityService> {
    let daemon = ServiceDaemon::new(IPMulticastTTLOption::LinkLocal).unwrap();
    let receiver = daemon.browse(MDNS_SERVICE_NAME).unwrap();
    let mut services = Vec::new();

    let deadline = Instant::now() + args.discovery_timeout.unwrap_or(Duration::from_millis(100));
    while let Ok(event) = receiver.recv_deadline(deadline) {
        if let ServiceEvent::ServiceResolved(info) = event {
            match filter_service(&info, &args) {
                Some((true, service)) => {
                    return vec![service];
                }
                Some((false, service)) => {
                    services.push(service);
                }
                None => {}
            }
        }
    }

    services
}

fn filter_service(info: &ServiceInfo, args: &DiscoveryArgs) -> Option<(bool, UnityService)> {
    let address = if let Some(ip) = info.get_addresses().iter().next() {
        SocketAddrV4::new(ip.to_owned(), info.get_port())
    } else {
        return None;
    };

    let path = if let Some(path) = info.get_property_val_str(PROJECT_PATH_PROP_KEY) {
        if let Ok(path) = PathBuf::from_str(path) {
            path
        } else {
            return None;
        }
    } else {
        return None;
    };

    let project = if let Some(project) = info.get_property_val_str(PROJECT_NAME_PROP_KEY) {
        project.to_owned()
    } else {
        return None;
    };

    let unity_version =
        if let Some(unity_version) = info.get_property_val_str(UNITY_VERSION_PROP_KEY) {
            unity_version.to_owned()
        } else {
            return None;
        };

    let session_name = info.get_fullname().replace(MDNS_SERVICE_NAME, "");

    let service = UnityService {
        address,
        hostname: info.get_hostname().to_owned(),
        path,
        project,
        unity_version,
        session_name,
    };

    if let Some(ref path_arg) = args.path {
        if let (Ok(path_arg), Ok(path)) = (
            std::fs::canonicalize(path_arg),
            std::fs::canonicalize(&service.path),
        ) {
            if path_arg == path {
                return Some((true, service));
            } else {
                return None;
            }
        }
    }

    if let Some(ref project_arg) = args.project {
        if !&service.project.starts_with(project_arg) {
            return None;
        } else {
            return Some((&service.project == project_arg, service));
        }
    }

    if let Some(ref session_arg) = args.session {
        if !&service.session_name.starts_with(session_arg) {
            return None;
        } else {
            return Some((&service.session_name == session_arg, service));
        }
    }

    Some((false, service))
}

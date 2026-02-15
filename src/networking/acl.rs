use ipnet::IpNet;
use std::net::IpAddr;
use std::str::FromStr;
use log::{warn, debug};

#[derive(Clone, Debug)]
pub struct AccessControl {
    allow_list: Vec<IpNet>,
    deny_list: Vec<IpNet>,
}

impl AccessControl {
    pub fn new(allow_strs: Option<Vec<String>>, deny_strs: Option<Vec<String>>) -> Self {
        let allow_list = parse_cidrs(allow_strs, "allow");
        let deny_list = parse_cidrs(deny_strs, "deny");
        
        AccessControl {
            allow_list,
            deny_list,
        }
    }

    pub fn is_allowed(&self, ip: IpAddr) -> bool {
        // 1. Check Deny List first (Blocklist)
        for net in &self.deny_list {
            if net.contains(&ip) {
                debug!("IP {} denied by explicit deny rule: {}", ip, net);
                return false;
            }
        }

        // 2. Check Allow List (Allowlist)
        if self.allow_list.is_empty() {
            // If no allow list is defined, default to ALLOW (unless denied above)
            return true;
        }

        for net in &self.allow_list {
            if net.contains(&ip) {
                debug!("IP {} allowed by rule: {}", ip, net);
                return true;
            }
        }

        // 3. Implicit Deny if allow list exists but no match
        debug!("IP {} denied (implicit deny - no matching allow rule)", ip);
        false
    }
}

fn parse_cidrs(input: Option<Vec<String>>, list_type: &str) -> Vec<IpNet> {
    match input {
        Some(strs) => strs.into_iter().filter_map(|s| {
            // Support both CIDR "1.2.3.0/24" and plain IP "1.2.3.4"
             match IpNet::from_str(&s) {
                 Ok(net) => Some(net),
                 Err(_) => {
                     // Try parsing as single IP, convert to /32 or /128
                     match s.parse::<IpAddr>() {
                         Ok(ip) => Some(IpNet::from(ip)),
                         Err(e) => {
                             warn!("Failed to parse {} list entry '{}': {}", list_type, s, e);
                             None
                         }
                     }
                 }
             }
        }).collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_allow_deny_logic() {
        // Deny 10.0.0.1, Allow 10.0.0.0/24
        let allow = Some(vec!["10.0.0.0/24".to_string()]);
        let deny = Some(vec!["10.0.0.1".to_string()]);
        let acl = AccessControl::new(allow, deny);

        // 10.0.0.1 -> Denied (Explicit Deny wins)
        assert!(!acl.is_allowed(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));

        // 10.0.0.2 -> Allowed (Matches Allow, not Deny)
        assert!(acl.is_allowed(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2))));

        // 192.168.1.1 -> Denied (Implicit Deny, not in allow list)
        assert!(!acl.is_allowed(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
    }

    #[test]
    fn test_no_lists() {
        let acl = AccessControl::new(None, None);
        // Default Allow
        assert!(acl.is_allowed(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4))));
    }
    
    #[test]
    fn test_deny_only() {
        // Only deny local
        let acl = AccessControl::new(None, Some(vec!["127.0.0.1".to_string()]));
        assert!(!acl.is_allowed(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(acl.is_allowed(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2))));
    }
}

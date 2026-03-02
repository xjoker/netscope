pub mod client;
pub mod dns;
pub mod egress;
pub mod geo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpFamily {
    V4,
    V6,
}

impl IpFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            IpFamily::V4 => "ipv4",
            IpFamily::V6 => "ipv6",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_family_v4_str() {
        assert_eq!(IpFamily::V4.as_str(), "ipv4");
    }

    #[test]
    fn ip_family_v6_str() {
        assert_eq!(IpFamily::V6.as_str(), "ipv6");
    }

    #[test]
    fn ip_family_equality() {
        assert_eq!(IpFamily::V4, IpFamily::V4);
        assert_ne!(IpFamily::V4, IpFamily::V6);
    }
}

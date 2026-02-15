use std::net::SocketAddr;
use bytes::{BufMut, BytesMut};

// Proxy Protocol V2 signature
const V2_SIG: [u8; 12] = [
    0x0D, 0x0A, 0x0D, 0x0A, 0x00, 0x0D, 0x0A, 0x51, 0x55, 0x49, 0x54, 0x0A,
];

pub fn create_v2_header(src_addr: SocketAddr, dst_addr: SocketAddr) -> Vec<u8> {
    let mut buf = BytesMut::with_capacity(128);

    // 1. Signature
    buf.put_slice(&V2_SIG);

    // 2. Version (2) | Command (PROXY = 1) -> 0x21
    buf.put_u8(0x21);

    // 3. Address Family & Transport Protocol
    match (src_addr, dst_addr) {
        (SocketAddr::V4(src), SocketAddr::V4(dst)) => {
            // AF_INET (1) | STREAM (1) -> 0x11
            buf.put_u8(0x11);
            // Length: 4 (src IP) + 4 (dst IP) + 2 (src port) + 2 (dst port) = 12 bytes
            buf.put_u16(12);
            
            buf.put_slice(&src.ip().octets());
            buf.put_slice(&dst.ip().octets());
            buf.put_u16(src.port());
            buf.put_u16(dst.port());
        }
        (SocketAddr::V6(src), SocketAddr::V6(dst)) => {
            // AF_INET6 (2) | STREAM (1) -> 0x21
            buf.put_u8(0x21);
            // Length: 16 (src IP) + 16 (dst IP) + 2 (src port) + 2 (dst port) = 36 bytes
            buf.put_u16(36);
            
            buf.put_slice(&src.ip().octets());
            buf.put_slice(&dst.ip().octets());
            buf.put_u16(src.port());
            buf.put_u16(dst.port());
        }
        _ => {
            // Mismatched families or UNIX socket (not supported here) -> Send "Unspec" (0x00)
            // Version 2 | Local (0) / Unspec (0) -> 0x20
            buf.put_u8(0x20); // LOCAL command
            buf.put_u8(0x00); // UNSPEC family / UNSPEC proto
            buf.put_u16(0);   // Length 0
        }
    }

    buf.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_v2_header_ipv4() {
        let src = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 12345);
        let dst = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 80);

        let header = create_v2_header(src, dst);

        // Sig (12) + Ver/Cmd (1) + Fam/Proto (1) + Len (2) + Addrs (12) = 28 bytes
        assert_eq!(header.len(), 28);
        assert_eq!(&header[0..12], &V2_SIG);
        assert_eq!(header[12], 0x21); // V2 PROXY
        assert_eq!(header[13], 0x11); // IPv4 TCP
        assert_eq!(header[14], 0x00); // Len high
        assert_eq!(header[15], 0x0C); // Len low (12)
        
        // Src IP (192.168.1.1)
        assert_eq!(&header[16..20], &[192, 168, 1, 1]);
        // Dst IP (10.0.0.1)
        assert_eq!(&header[20..24], &[10, 0, 0, 1]);
        // Src Port (12345 = 0x3039)
        assert_eq!(&header[24..26], &[0x30, 0x39]);
        // Dst Port (80 = 0x0050)
        assert_eq!(&header[26..28], &[0x00, 0x50]);
    }
}

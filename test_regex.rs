fn main() {
    let raw_lines = vec![
        "1131525912 2005.11.09 tbird-admin1 Nov 10 00:45:12 local@tbird-admin1 postfix/postdrop[11735]: warning: unable to look up public/pickup: No such file or directory",
        "1135628543 2005.12.26 dn866 Dec 26 12:22:23 dn866/dn866 kernel: MOSAL(1): mnt_projects/sysapps/src/ib/topspin/topspin-src-3.2.0-16/third_party/thca4_linux/kernel/mlxsys/obj_host_amd64_custom1_rhel4/mlxsys/mosal_iobuf.c[126]: dump iobuf at 00000101a54bc080 :",
        "1135628543 2005.12.26 dn866 Dec 26 12:22:23 dn866/dn866 kernel: MOSAL(1): need_page_secure = no"
    ];

    let re = regex::Regex::new(r"^(?P<ts>\d{10})\s+(?P<date>\d{4}\.\d{2}\.\d{2})\s+(?P<host1>\S+)\s+(?P<syslog_date>[A-Z][a-z]{2}\s+\d{1,2})\s+(?P<syslog_time>\d{2}:\d{2}:\d{2})\s+(?P<host2>\S+)\s+(?P<app>[^:]+):\s+(?:(?P<lvl>warning|error|info|fatal|debug):\s+)?(?P<body>.*)$").unwrap();

    for line in raw_lines {
        if let Some(caps) = re.captures(line) {
            println!("MATCH: ts={} lvl={:?} body={}", &caps["ts"], caps.name("lvl").map(|m| m.as_str()), &caps["body"]);
        } else {
            println!("NO MATCH: {}", line);
        }
    }
}

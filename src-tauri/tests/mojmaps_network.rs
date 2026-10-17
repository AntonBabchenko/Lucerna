//! The srgutils port against Forge's declared MOJMAPS_SHA for three Minecraft
//! versions (values copied from the Forge installers' install_profile.json:
//! 1.20.6-50.2.0, 1.21.1-52.1.14, 1.21.11-61.2.0). Network; ignored in CI.

#[tokio::test]
#[ignore]
async fn sanitized_mojmaps_match_forges_declared_sha() {
    let cases = [
        ("1.20.6", "269c6b975e8731ae55c12caddecb257c6fe25a22"),
        ("1.21.1", "2a8064cf9359dd03125bc6f4aa99715dbbf50492"),
        ("1.21.11", "f9240aaa50a06c04d8e285772ba16a340ddc5aef"),
    ];
    for (mc, want) in cases {
        let td = tempfile::tempdir().unwrap();
        let out = td.path().join("m.tsrg");
        let args: Vec<String> = [
            "--task",
            "DOWNLOAD_MOJMAPS",
            "--sanitize",
            "--version",
            mc,
            "--side",
            "client",
            "--output",
        ]
        .iter()
        .map(|s| s.to_string())
        .chain([out.display().to_string()])
        .collect();
        lucerna_lib::forge::patcher::mojmaps::run("net.minecraftforge:installertools:1.4.3", &args)
            .await
            .unwrap_or_else(|e| panic!("{mc}: {e:?}"));
        let got = lucerna_lib::mods::txn::file_sha1(&out)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got, want, "{mc}");
    }
}

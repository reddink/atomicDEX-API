fn main() {
    let mut prost_utxo = prost_build::Config::new();
    prost_utxo.out_dir("utxo");
    prost_utxo.compile_protos(&["utxo/bchrpc.proto"], &["utxo"]).unwrap();

    let mut prost_z_coin = prost_build::Config::new();
    prost_z_coin.out_dir("z_coin");
    prost_z_coin
        .compile_protos(&["z_coin/service.proto"], &["z_coin"])
        .unwrap();

    tonic_build::configure()
        .build_server(false)
        .compile(&["z_coin/service.proto"], &["z_coin"])
        .unwrap();
}

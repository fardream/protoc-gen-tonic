use std::{
    collections::HashMap,
    fs::{read, File},
    io::{stdin, Read, Write},
    path::PathBuf,
};

use clap::Parser;
use prost::Message;
use prost_build::{Config, Module};
use prost_types::FileDescriptorSet;

/// protoc-gen-tonic is a proto plugin that generate prost and tonic code.
/// The output file can either source relative or specified by options (all relative to the output directory).
#[derive(Parser, Debug)]
struct Args {
    /// input is path to the FileDescriptorSet containing all the proto informations to generate for.
    #[arg(long, short)]
    input: String,
    /// output is the path to generated output file
    #[arg(long, short)]
    output: Option<PathBuf>,

    /// extern_path maps a proto package to a rust import path.
    /// To map a package from `a.b.c` to `x::y`, use `.a.b.c=::x::y` - notice the leading `.` and `::`.
    #[arg(long)]
    extern_path: Vec<String>,

    /// add attribute to field. In the form of `path=attribute`.
    /// For example, to add `serde` to field `f` in Message `M` defined in package `a.b`,
    /// use  `.a.b.M.f=#[derive(serde::Serialize, serde::Deserialize)]`
    #[arg(long)]
    field_attribute: Vec<String>,
    /// add attribute to types. In the form of `path=attribute`.
    #[arg(long)]
    type_attribute: Vec<String>,
    /// add attribute to a message/`struct`. In the form of `path=attribute`.
    #[arg(long)]
    message_attribute: Vec<String>,
    /// add attribute to an enum/`struct`. In the form of `path=attribute`.
    #[arg(long)]
    enum_attribute: Vec<String>,
    /// add attribute to tonic client.
    #[arg(long)]
    client_attribute: Vec<String>,
    /// add attribute to tonic server.
    #[arg(long)]
    server_attribute: Vec<String>,

    /// module specific output map.
    #[arg(long)]
    module_output_map: Vec<String>,

    #[arg(long)]
    source_relative: bool,
}

fn split_arg(s: &str) -> (&str, &str) {
    let segs = s.splitn(2, '=').collect::<Vec<_>>();
    if segs.len() != 2 {
        panic!("argument {} is not in the form of a=b", s);
    }

    (segs[0], segs[1])
}

fn main() {
    let args = Args::parse();

    let mut prost_config = Config::new();
    let mut tonic_build = tonic_build::configure();

    for x in args.extern_path.iter() {
        let (a, b) = split_arg(x);
        prost_config.extern_path(a, b);
        tonic_build = tonic_build.extern_path(a, b);
    }

    for x in args.field_attribute.iter() {
        let (a, b) = split_arg(x);
        prost_config.field_attribute(a, b);
        tonic_build = tonic_build.field_attribute(a, b);
    }
    for x in args.type_attribute.iter() {
        let (a, b) = split_arg(x);
        prost_config.type_attribute(a, b);
        tonic_build = tonic_build.type_attribute(a, b);
    }
    for x in args.message_attribute.iter() {
        let (a, b) = split_arg(x);
        prost_config.message_attribute(a, b);
        tonic_build = tonic_build.message_attribute(a, b);
    }
    for x in args.enum_attribute.iter() {
        let (a, b) = split_arg(x);
        prost_config.enum_attribute(a, b);
        tonic_build = tonic_build.enum_attribute(a, b);
    }
    for x in args.server_attribute.iter() {
        let (a, b) = split_arg(x);
        tonic_build = tonic_build.server_attribute(a, b);
    }
    for x in args.client_attribute.iter() {
        let (a, b) = split_arg(x);
        tonic_build = tonic_build.client_attribute(a, b);
    }

    prost_config.skip_protoc_run();
    tonic_build = tonic_build.skip_protoc_run();

    prost_config.service_generator(tonic_build.service_generator());

    let buf = if args.input == "-" {
        let mut buf = Vec::new();
        stdin().read_to_end(&mut buf).unwrap();
        buf
    } else {
        read(&args.input).unwrap()
    };

    let file_descriptor_set = FileDescriptorSet::decode(&*buf).unwrap();

    let request = file_descriptor_set
        .file
        .into_iter()
        .map(|d| (Module::from_protobuf_package_name(d.package()), d))
        .collect();

    let modules = prost_config.generate(request).unwrap();

    let mut output_file: Option<File> = None;

    let mut module_output_map: HashMap<Module, PathBuf> = HashMap::new();
    for x in args.module_output_map.iter() {
        let (module_name, output_file_name) = split_arg(x);
        module_output_map.insert(
            Module::from_protobuf_package_name(module_name),
            PathBuf::from(output_file_name),
        );
    }

    for (module, content) in &modules {
        match module_output_map.get(module) {
            Some(p) => {
                let mut output = File::create(p).unwrap();
                writeln!(output, "{}", content).unwrap();
            }
            None => {
                if output_file.is_none() {
                    if args.output.is_none() {
                        panic!("module {} has no output", module);
                    }
                    output_file = Some(File::create(args.output.as_ref().unwrap()).unwrap());
                }

                writeln!(
                    output_file.as_ref().unwrap(),
                    "// module: {}\n{}",
                    module,
                    content
                )
                .unwrap();
            }
        }
    }
}

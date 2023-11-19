use std::{
    collections::HashMap,
    fs::{create_dir_all, read, File},
    io::{stdin, Read, Write},
    path::PathBuf,
};

use anyhow::Context;
use clap::Parser;
use prost::Message;
use prost_build::{Config, Module};
use prost_reflect::DescriptorPool;
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

    /// module a specific input file to a specific output file
    /// the map should be in the format of `path/to/input.proto=path/to/output.rs`.
    #[arg(long)]
    output_map: Vec<String>,

    /// add additional module declarations in the file. the input proto is identified by its path,
    /// in the format of `path/to/input.proto=a::b::c`.
    #[arg(long)]
    module_in_file: Vec<String>,

    /// create directories
    #[arg(long)]
    create_directory: bool,

    /// bytes with the descriptor bin data for proto-reflect
    /// for example, if the bytes will be defined in `lib.rs` and named `PROTO_DEF`,
    /// the value should be `crate::PROTO_DEF`.
    #[arg(long)]
    proto_reflect_byte: Option<String>,
}

fn split_arg(s: &str) -> (&str, &str) {
    let segs = s.splitn(2, '=').collect::<Vec<_>>();
    if segs.len() != 2 {
        panic!("argument {} is not in the form of a=b", s);
    }

    (segs[0], segs[1])
}

fn write_with_module(f: &mut impl Write, content: &str, modules: &[&str]) {
    for x in modules.iter() {
        writeln!(f, "pub mod {} {{", x).unwrap();
    }
    writeln!(f, "{}", content).unwrap();
    for _ in modules.iter() {
        writeln!(f, "}}").unwrap();
    }
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
        stdin()
            .read_to_end(&mut buf)
            .context("failed to read from stdin")
            .unwrap();
        buf
    } else {
        read(&args.input)
            .with_context(|| format!("failed to read input file {}", args.input))
            .unwrap()
    };

    let file_descriptor_set = FileDescriptorSet::decode(&*buf).unwrap();

    if let Some(proto_reflect_bytes) = args.proto_reflect_byte {
        let descriptor = DescriptorPool::decode(&*buf).unwrap();
        let pool_attribute = format!(
            r#"#[prost_reflect(file_descriptor_set_bytes = "{}")]"#,
            proto_reflect_bytes,
        );
        for message in descriptor.all_messages() {
            let full_name = message.full_name();
            prost_config
                .type_attribute(full_name, "#[derive(::prost_reflect::ReflectMessage)]")
                .type_attribute(
                    full_name,
                    &format!(r#"#[prost_reflect(message_name = "{}")]"#, full_name,),
                )
                .type_attribute(full_name, &pool_attribute);
        }
    }

    let mut module_to_input: HashMap<Module, &str> = HashMap::new();

    let request = file_descriptor_set
        .file
        .iter()
        .map(|d| {
            let m = Module::from_protobuf_package_name(d.package());
            if module_to_input.contains_key(&m) {
                panic!("module duplicate: {}", m);
            }

            module_to_input.insert(m.clone(), d.name());
            (m, d.to_owned())
        })
        .collect();

    let modules = prost_config.generate(request).unwrap();

    let mut output_file: Option<File> = None;

    let mut output_map: HashMap<&str, PathBuf> = HashMap::new();
    for x in args.output_map.iter() {
        let (input_file, output_file_name) = split_arg(x);
        output_map.insert(input_file, PathBuf::from(output_file_name));
    }

    let mut module_in_file_map: HashMap<&str, Vec<&str>> = HashMap::new();
    for x in args.module_in_file.iter() {
        let (input_file, add_module) = split_arg(x);
        module_in_file_map.insert(input_file, add_module.split("::").collect());
    }

    for (module, content) in &modules {
        let input_file = module_to_input.get(module).unwrap();
        let modules_in_file = match module_in_file_map.get(input_file) {
            Some(x) => x.clone(),
            None => vec![],
        };

        match output_map.get(input_file) {
            Some(p) => {
                if args.create_directory {
                    if let Some(parent) = p.parent() {
                        create_dir_all(parent)
                            .with_context(|| format!("failed to create directory {:?}", parent))
                            .unwrap();
                    }
                }
                let mut output = File::create(p)
                    .with_context(|| format!("failed to create file {:?}", p))
                    .unwrap();
                write_with_module(&mut output, content, &modules_in_file);
            }
            None => {
                if output_file.is_none() {
                    if args.output.is_none() {
                        panic!("module {} has no output", module);
                    }
                    let output_path = args.output.as_ref().unwrap();
                    if args.create_directory {
                        if let Some(parent) = output_path.parent() {
                            create_dir_all(parent)
                                .with_context(|| format!("failed to create directory {:?}", parent))
                                .unwrap();
                        }
                    }
                    output_file = Some(File::create(output_path).unwrap());
                }
                write_with_module(
                    &mut output_file.as_ref().unwrap(),
                    content,
                    &modules_in_file,
                );
            }
        }
    }
}

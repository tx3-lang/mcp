use cynic_codegen;

fn main() {
  cynic_codegen::register_schema("tx3")
    .from_sdl_file("schema.graphql")
    .unwrap()
    .as_default()
    .unwrap();
}
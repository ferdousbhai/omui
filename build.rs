fn main() {
    glib_build_tools::compile_resources(&["data"], "data/omui.gresource.xml", "omui.gresource");
}

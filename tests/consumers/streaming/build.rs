fn main() {
    crucible_build::model("../../models/server-stream")
        .server()
        .custom_rejections()
        .generate();
}

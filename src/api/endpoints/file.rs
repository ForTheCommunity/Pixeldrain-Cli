use reqwest::{
    Body, Method,
    multipart::{Form, Part},
};
use tokio::io::AsyncRead;
use tokio_util::io::ReaderStream;

use crate::api::{client::ApiClient, error::ApiResult, models::upload::UploadResponse};

impl ApiClient {
    // reads whole file and uploads it (For Small Files)
    pub async fn upload_file_bytes(
        &self,
        filename: &str,
        bytes: Vec<u8>,
    ) -> ApiResult<UploadResponse> {
        let part = Part::bytes(bytes).file_name(filename.to_string());
        let form = Form::new().part("file", part);

        let request_builder = self.request(Method::POST, "/api/file").multipart(form);
        self.send(request_builder).await
    }

    // Stream file from any AsyncRead reader.
    pub async fn upload_file_stream<R: AsyncRead + Send + Unpin + 'static>(
        &self,
        filename: &str,
        reader: R,
        length: u64,
    ) -> ApiResult<UploadResponse> {
        let stream = ReaderStream::new(reader);
        let body = Body::wrap_stream(stream);
        let part = Part::stream_with_length(body, length).file_name(filename.to_string());
        let form = Form::new().part("file", part);

        let req_builder = self.request(Method::POST, "/api/file").multipart(form);
        self.send(req_builder).await
    }

    // pub async fn get_file_info(&self, file_id: &str) -> ApiResult<FileInfo> {
    //     let endpoint = format!("/api/file/{}/info", file_id);
    //     let req = self.request(Method::GET, &endpoint);
    //     self.send(req).await
    // }
}

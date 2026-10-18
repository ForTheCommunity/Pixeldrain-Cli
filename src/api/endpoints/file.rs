use reqwest::{
    Body, Method,
    multipart::{Form, Part},
};
use tokio::io::AsyncRead;
use tokio_util::io::{ReaderStream, StreamReader};

use crate::api::{
    client::ApiClient, error::{ApiError, ApiResult}, models::{FileInfo, upload::UploadResponse},
};
use futures_util::TryStreamExt;

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

    pub async fn download_file_stream(
        &self,
        file_id: &str,
    ) -> ApiResult<(u64, impl AsyncRead + Unpin + Send)> {
        let url = format!("/api/file/{}", file_id);
        let req_builder = self.request(Method::GET, &url);

        let response = self.send_response(req_builder).await?;
        let length = response.content_length().unwrap_or(0);

        let byte_stream = response
            .bytes_stream()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));

        let async_reader = StreamReader::new(byte_stream);

        Ok((length, async_reader))
    }

    pub async fn send_response(
        &self,
        req_builder: reqwest::RequestBuilder,
    ) -> ApiResult<reqwest::Response> {
        let response = req_builder.send().await?;
        let status = response.status();

        // If the server returns a non-success status code (e.g., 400, 401, 404, 500)
        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error body".to_string());

            if status == reqwest::StatusCode::UNAUTHORIZED {
                return Err(ApiError::Unauthorized(message));
            }

            return Err(ApiError::HttpStatusError { status, message });
        }

        Ok(response)
    }

    pub async fn get_file_info(&self, file_id: &str) -> ApiResult<FileInfo> {
        let endpoint = format!("/api/file/{}/info", file_id);
        let req = self.request(Method::GET, &endpoint);
        self.send(req).await
    }
}

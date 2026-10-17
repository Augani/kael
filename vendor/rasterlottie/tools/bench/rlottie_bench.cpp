#include <rlottie.h>

#include <chrono>
#include <cmath>
#include <cstdint>
#include <iomanip>
#include <iostream>
#include <memory>
#include <string>
#include <vector>

namespace {

enum class BenchmarkMode {
    Full,
    Gif,
};

BenchmarkMode parse_mode(const std::string& value) {
    if (value == "full") {
        return BenchmarkMode::Full;
    }
    if (value == "gif") {
        return BenchmarkMode::Gif;
    }
    throw std::runtime_error("unsupported mode: " + value);
}

std::string mode_name(BenchmarkMode mode) {
    switch (mode) {
    case BenchmarkMode::Full:
        return "full";
    case BenchmarkMode::Gif:
        return "gif";
    }

    return "unknown";
}

}  // namespace

int main(int argc, char** argv) {
    if (argc < 5) {
        std::cerr << "usage: rlottie_bench <json-path> <full|gif> <max-fps> <scale>\n";
        return 1;
    }

    const std::string json_path = argv[1];
    const BenchmarkMode mode = parse_mode(argv[2]);
    const double max_fps = std::stod(argv[3]);
    const double scale = std::stod(argv[4]);

    auto animation = rlottie::Animation::loadFromFile(json_path);
    if (!animation) {
        std::cerr << "failed to load animation: " << json_path << "\n";
        return 2;
    }

    size_t source_width = 0;
    size_t source_height = 0;
    animation->size(source_width, source_height);
    const size_t width = std::max<size_t>(
        static_cast<size_t>(std::ceil(static_cast<double>(source_width) * scale)), 1);
    const size_t height = std::max<size_t>(
        static_cast<size_t>(std::ceil(static_cast<double>(source_height) * scale)), 1);
    const double source_fps = std::max(animation->frameRate(), 1.0);
    const size_t total_frames = std::max<size_t>(animation->totalFrame(), 1);

    double requested_fps = source_fps;
    double actual_output_fps = source_fps;
    size_t rendered_frames = total_frames;

    if (mode == BenchmarkMode::Gif) {
        requested_fps = std::min(source_fps, std::max(max_fps, 1.0));
        const auto frame_delay =
            std::max<uint16_t>(static_cast<uint16_t>(std::llround(100.0 / requested_fps)), 1);
        actual_output_fps = 100.0 / static_cast<double>(frame_delay);
        rendered_frames = std::max<size_t>(
            static_cast<size_t>(
                std::floor(actual_output_fps * (static_cast<double>(total_frames) / source_fps))),
            1);
    }

    const double source_frame_step = source_fps / actual_output_fps;
    std::vector<uint32_t> buffer(width * height);
    rlottie::Surface surface(buffer.data(), width, height, width * sizeof(uint32_t));

    const auto start = std::chrono::steady_clock::now();
    for (size_t rendered = 0; rendered < rendered_frames; ++rendered) {
        size_t source_frame = rendered;
        if (mode == BenchmarkMode::Gif) {
            source_frame = static_cast<size_t>(std::llround(rendered * source_frame_step));
        }
        if (source_frame >= total_frames) {
            source_frame = total_frames - 1;
        }
        animation->renderSync(source_frame, surface, true);
    }
    const auto end = std::chrono::steady_clock::now();
    const auto elapsed_us =
        std::chrono::duration_cast<std::chrono::microseconds>(end - start).count();
    const double elapsed_ms = static_cast<double>(elapsed_us) / 1000.0;
    const double avg_ms_per_frame = elapsed_ms / static_cast<double>(rendered_frames);

    std::cout << std::fixed << std::setprecision(6);
    std::cout << "{"
              << "\"renderer\":\"rlottie\","
              << "\"mode\":\"" << mode_name(mode) << "\","
              << "\"source_fps\":" << source_fps << ","
              << "\"requested_fps\":" << requested_fps << ","
              << "\"actual_output_fps\":" << actual_output_fps << ","
              << "\"start_frame\":0,"
              << "\"end_frame\":" << total_frames << ","
              << "\"rendered_frames\":" << rendered_frames << ","
              << "\"output_width\":" << width << ","
              << "\"output_height\":" << height << ","
              << "\"scale\":" << scale << ","
              << "\"elapsed_ms\":" << elapsed_ms << ","
              << "\"avg_ms_per_frame\":" << avg_ms_per_frame << "}\n";
    return 0;
}

def metrics(key; name; value): {
  key: "\(key)-\($os)",
  name: "\(name) relative to Ninja on \($os)",
  metrics: map({
    key: .turtle.name,
    value: ((.turtle | value) / (.ninja | value))
  })
};

[inputs.results[] | .summary + (.name | capture("(?<tool>.+) \\((?<name>.+)\\)"))]
| group_by(.name)
| map(INDEX(.tool))
| metrics("time"; "build time"; .time_wall_clock.mean),
  metrics("memory"; "peak memory usage"; .memory_peak_resident.max)

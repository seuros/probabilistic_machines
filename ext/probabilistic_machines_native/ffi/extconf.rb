# frozen_string_literal: true

require 'mkmf'
require 'pathname'
require 'rb_sys/mkmf'

create_rust_makefile('probabilistic_machines_native/probabilistic_machines_native') do |r|
  r.ext_dir = Pathname(__dir__).relative_path_from(Pathname(Dir.pwd)).to_s
  r.profile = ENV.fetch('RB_SYS_CARGO_PROFILE', :release).to_sym
end

# rake-compiler builds from tmp/, where the generated relative manifest path no
# longer resolves.
makefile = File.join(Dir.pwd, 'Makefile')
File.write(
  makefile,
  File.read(makefile).gsub(
    /^RB_SYS_CARGO_MANIFEST_DIR \?=.*$/,
    "RB_SYS_CARGO_MANIFEST_DIR ?= #{File.expand_path(__dir__)}"
  )
)

# frozen_string_literal: true

require_relative 'probabilistic_machines/version'

module ProbabilisticMachines
  # The core's `Hitter`: a tracked key, its counter, and how much of that
  # counter it may have inherited. The true count lies in
  # `count - overestimate` through `count`, so like `Hitter::new` it needs an
  # overestimate below the count: anything else raises RangeError (TypeError
  # for a key that is not a String or a count that is not an Integer).
  Hitter = Data.define(:key, :count, :overestimate) do
    def initialize(key:, count:, overestimate:)
      key, count, overestimate = ProbabilisticMachines.__send__(:__hitter, key, count, overestimate)
      super
    end
  end

  # The core's `Centroid`: one cluster of a digest, the mean of the values that
  # collapsed into it. Like `Centroid::new` it holds a finite mean and a
  # finite, positive weight: anything else raises RangeError (TypeError for a
  # non-number). Stored as Floats.
  Centroid = Data.define(:mean, :weight) do
    def initialize(mean:, weight:)
      mean, weight = ProbabilisticMachines.__send__(:__centroid, mean, weight)
      super
    end
  end
end

# Native is required: there is no pure-Ruby fallback. It defines every sketch
# class and validates the Data values above.
require 'probabilistic_machines_native/probabilistic_machines_native'

module ProbabilisticMachines
  private_class_method :__hitter, :__centroid

  # The values are final. `initialize` is where each validates, and a subclass
  # could override it, or `allocate` build an instance that never ran it:
  # subclassing raises TypeError (as subclassing Class does) and `allocate`
  # is gone. `new`, keywords, `with` and `dup` all still run the check.
  [Hitter, Centroid].each do |value|
    value.singleton_class.undef_method(:allocate)
    value.define_singleton_method(:inherited) { |_| raise TypeError, "#{value} cannot be subclassed" }
  end
end

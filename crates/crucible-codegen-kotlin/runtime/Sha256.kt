/** SHA-256 over the final wire bytes, shared by every platform. */
internal fun sha256(input: ByteArray): String {
    val constants = uintArrayOf(
        0x428a2f98u,0x71374491u,0xb5c0fbcfu,0xe9b5dba5u,0x3956c25bu,0x59f111f1u,0x923f82a4u,0xab1c5ed5u,
        0xd807aa98u,0x12835b01u,0x243185beu,0x550c7dc3u,0x72be5d74u,0x80deb1feu,0x9bdc06a7u,0xc19bf174u,
        0xe49b69c1u,0xefbe4786u,0x0fc19dc6u,0x240ca1ccu,0x2de92c6fu,0x4a7484aau,0x5cb0a9dcu,0x76f988dau,
        0x983e5152u,0xa831c66du,0xb00327c8u,0xbf597fc7u,0xc6e00bf3u,0xd5a79147u,0x06ca6351u,0x14292967u,
        0x27b70a85u,0x2e1b2138u,0x4d2c6dfcu,0x53380d13u,0x650a7354u,0x766a0abbu,0x81c2c92eu,0x92722c85u,
        0xa2bfe8a1u,0xa81a664bu,0xc24b8b70u,0xc76c51a3u,0xd192e819u,0xd6990624u,0xf40e3585u,0x106aa070u,
        0x19a4c116u,0x1e376c08u,0x2748774cu,0x34b0bcb5u,0x391c0cb3u,0x4ed8aa4au,0x5b9cca4fu,0x682e6ff3u,
        0x748f82eeu,0x78a5636fu,0x84c87814u,0x8cc70208u,0x90befffau,0xa4506cebu,0xbef9a3f7u,0xc67178f2u)
    val state = uintArrayOf(0x6a09e667u,0xbb67ae85u,0x3c6ef372u,0xa54ff53au,0x510e527fu,0x9b05688cu,0x1f83d9abu,0x5be0cd19u)
    val length = input.size.toLong() * 8
    val padded = input.copyOf(((input.size.toLong() + 72) / 64 * 64).also { require(it <= Int.MAX_VALUE) }.toInt())
    padded[input.size] = 0x80.toByte()
    repeat(8) { padded[padded.size - 1 - it] = (length ushr (8 * it)).toByte() }
    val words = UIntArray(64)
    for (offset in padded.indices step 64) {
        repeat(16) { index -> words[index] = (0..3).fold(0u) { word, byte -> (word shl 8) or (padded[offset + index * 4 + byte].toUInt() and 255u) } }
        for (i in 16..63) {
            val a = words[i-15]; val b = words[i-2]
            words[i] = words[i-16] + (a.rotateRight(7) xor a.rotateRight(18) xor (a shr 3)) + words[i-7] + (b.rotateRight(17) xor b.rotateRight(19) xor (b shr 10))
        }
        var a=state[0]; var b=state[1]; var c=state[2]; var d=state[3]
        var e=state[4]; var f=state[5]; var g=state[6]; var h=state[7]
        repeat(64) { i ->
            val t1=h+(e.rotateRight(6) xor e.rotateRight(11) xor e.rotateRight(25))+((e and f) xor (e.inv() and g))+constants[i]+words[i]
            val t2=(a.rotateRight(2) xor a.rotateRight(13) xor a.rotateRight(22))+((a and b) xor (a and c) xor (b and c))
            h=g; g=f; f=e; e=d+t1; d=c; c=b; b=a; a=t1+t2
        }
        state[0]+=a;state[1]+=b;state[2]+=c;state[3]+=d;state[4]+=e;state[5]+=f;state[6]+=g;state[7]+=h
    }
    return state.joinToString("") { it.toString(16).padStart(8,'0') }
}

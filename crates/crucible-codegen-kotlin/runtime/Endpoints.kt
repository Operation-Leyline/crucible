import io.ktor.http.Url

/** Validates the configured endpoint: an absolute HTTP(S) base URL with a well-formed host. */
internal object Endpoints {
    private val label=Regex("[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?")
    private fun hostLabel(value:String,subdomains:Boolean)=value.isNotEmpty() && (if(subdomains) value.split('.') else listOf(value)).all { label.matches(it) }
    private fun ipv6(value:String):Boolean {
        if(value.count { it==':' }<2 || value.count { it=='%' }>0 || value.contains(":::")) return false
        val halves=value.split("::"); if(halves.size>2) return false
        val parts=halves.flatMap { it.split(':').filter(String::isNotEmpty) }
        var count=0
        for((index,part) in parts.withIndex()) {
            if('.' in part) { if(index!=parts.lastIndex || part.split('.').size!=4 || part.split('.').any { it.isEmpty() || it.any { c-> !c.isDigit() } || (it.toIntOrNull() ?: -1) !in 0..255 }) return false; count+=2 }
            else { if(part.length !in 1..4 || part.any { it !in "0123456789abcdefABCDEF" }) return false; count++ }
        }
        return if(halves.size==2) count<8 else count==8
    }
    fun valid(value:String):Boolean {
        if(value.any { it.code<=32 || it.code==127 } || '?' in value || '#' in value || '\\' in value || Regex("%(?![A-Fa-f0-9]{2})").containsMatchIn(value)) return false
        return try {
            if(!value.startsWith("https://") && !value.startsWith("http://")) return false
            val url=Url(value); val host=url.host.removeSurrounding("[","]")
            if(host.isEmpty() || url.user!=null || url.password!=null || url.port !in 1..65535) return false
            val address=if(':' in host) ipv6(host) else host.split('.').let { it.size==4 && it.all { s->s.isNotEmpty() && s.all(Char::isDigit) && (s.toIntOrNull() ?: -1) in 0..255 } }
            if(!address && !hostLabel(host.trimEnd('.'),true)) return false
            true
        } catch (_:Exception) { false }
    }
}
